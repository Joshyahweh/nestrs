//! Production Kafka transport using [rskafka](https://docs.rs/rskafka) (pure Rust).
//!
//! Wire format matches Redis/NATS: JSON `WireRequest` payloads on the `requests` topic; replies go to
//! a per-client `replies.{instance_id}` topic with record key = `correlation_id`.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::connection::client_builder_from_parts;
use async_trait::async_trait;
use chrono::Utc;
use rskafka::client::partition::{Compression, OffsetAt, UnknownTopicHandling};
use rskafka::record::Record;
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::connection::KafkaConnectionOptions;
use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, MicroserviceServer, ShutdownFuture, Transport, TransportError};

/// Client / producer options.
///
/// **Topic retention**: the rskafka `create_topic` helper does not set `retention.ms`. Configure retention via
/// broker defaults, `kafka-topics --alter`, or your cluster operator (Strimzi, MSK, Confluent, etc.).
#[derive(Clone, Debug)]
pub struct KafkaTransportOptions {
    pub bootstrap_brokers: Vec<String>,
    pub topic_prefix: String,
    pub request_timeout: std::time::Duration,
    pub replication_factor: i16,
    pub create_topics: bool,
    /// TLS (recommended with SASL on untrusted networks), SASL, and Kafka `client.id`.
    pub connection: KafkaConnectionOptions,
}

impl Default for KafkaTransportOptions {
    fn default() -> Self {
        Self {
            bootstrap_brokers: vec!["127.0.0.1:9092".to_string()],
            topic_prefix: "nestrs".to_string(),
            request_timeout: std::time::Duration::from_secs(30),
            replication_factor: 1i16,
            create_topics: true,
            connection: KafkaConnectionOptions::default(),
        }
    }
}

impl KafkaTransportOptions {
    pub fn new(brokers: Vec<String>) -> Self {
        Self {
            bootstrap_brokers: brokers,
            ..Default::default()
        }
    }

    fn requests_topic(&self) -> String {
        format!("{}.requests", self.topic_prefix)
    }

    fn replies_topic(&self, instance_id: &str) -> String {
        format!("{}.replies.{}", self.topic_prefix, instance_id)
    }
}

/// Nest-style Kafka [`Transport`] (JSON payloads, same patterns as Redis pub/sub).
pub struct KafkaTransport {
    options: KafkaTransportOptions,
    instance_id: String,
    client: Mutex<Option<Arc<rskafka::client::Client>>>,
}

impl KafkaTransport {
    pub fn new(options: KafkaTransportOptions) -> Self {
        Self {
            instance_id: Uuid::new_v4().simple().to_string(),
            options,
            client: Mutex::new(None),
        }
    }

    async fn connect(&self) -> Result<Arc<rskafka::client::Client>, TransportError> {
        let mut g = self.client.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let builder = client_builder_from_parts(
            self.options.bootstrap_brokers.clone(),
            &self.options.connection,
        )
        .map_err(|e| TransportError::new(format!("kafka client options: {e}")))?;
        let c = Arc::new(
            builder
                .build()
                .await
                .map_err(|e| TransportError::new(format!("kafka connect failed: {e}")))?,
        );
        if self.options.create_topics {
            if let Ok(ctrl) = c.controller_client() {
                let _ = ctrl
                    .create_topic(
                        self.options.requests_topic(),
                        1,
                        self.options.replication_factor,
                        5_000,
                    )
                    .await;
                let _ = ctrl
                    .create_topic(
                        self.options.replies_topic(&self.instance_id),
                        1,
                        self.options.replication_factor,
                        5_000,
                    )
                    .await;
            }
        }
        *g = Some(c.clone());
        Ok(c)
    }

    async fn partition(
        &self,
        topic: &str,
    ) -> Result<rskafka::client::partition::PartitionClient, TransportError> {
        let c = self.connect().await?;
        c.partition_client(topic.to_owned(), 0, UnknownTopicHandling::Retry)
            .await
            .map_err(|e| {
                TransportError::new(format!("kafka partition client `{topic}` failed: {e}"))
            })
    }
}

#[async_trait]
impl Transport for KafkaTransport {
    async fn send_json(&self, pattern: &str, payload: Value) -> Result<Value, TransportError> {
        let correlation_id = Uuid::new_v4().simple().to_string();
        let reply_topic = self.options.replies_topic(&self.instance_id);
        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply_topic.clone()),
            correlation_id: Some(correlation_id.clone()),
        };
        let body = serde_json::to_vec(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let req_pc = self.partition(&self.options.requests_topic()).await?;
        let rep_pc = self.partition(&reply_topic).await?;

        let start_off = rep_pc
            .get_offset(OffsetAt::Latest)
            .await
            .map_err(|e| TransportError::new(format!("kafka get_offset (replies) failed: {e}")))?;

        let record = Record {
            key: None,
            value: Some(body),
            headers: BTreeMap::new(),
            timestamp: Utc::now(),
        };
        req_pc
            .produce(vec![record], Compression::default())
            .await
            .map_err(|e| TransportError::new(format!("kafka produce failed: {e}")))?;
        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_kafka_produce_total", "topic" => "requests")
            .increment(1);

        let deadline = tokio::time::Instant::now() + self.options.request_timeout;
        let mut next_off = start_off;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(TransportError::new("kafka request timed out"));
            }
            let (records, _) = rep_pc
                .fetch_records(next_off, 1..1_000_000, 500)
                .await
                .map_err(|e| TransportError::new(format!("kafka fetch (replies) failed: {e}")))?;

            if records.is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                continue;
            }

            for ro in records {
                next_off = ro.offset + 1;
                let key_bytes = ro.record.key.as_deref().unwrap_or_default();
                let key_str = String::from_utf8_lossy(key_bytes);
                if key_str != correlation_id {
                    continue;
                }
                let val = ro
                    .record
                    .value
                    .as_deref()
                    .ok_or_else(|| TransportError::new("kafka reply missing value"))?;
                let wire: WireResponse = serde_json::from_slice(val).map_err(|e| {
                    TransportError::new(format!("deserialize response failed: {e}"))
                })?;
                if wire.ok {
                    return Ok(wire.payload.unwrap_or(Value::Null));
                }
                let err = wire.error.unwrap_or(WireError {
                    message: "microservice error".to_string(),
                    details: None,
                });
                let mut out = TransportError::new(err.message);
                if let Some(details) = err.details {
                    out = out.with_details(details);
                }
                return Err(out);
            }
        }
    }

    async fn emit_json(&self, pattern: &str, payload: Value) -> Result<(), TransportError> {
        let wire = WireRequest {
            kind: WireKind::Emit,
            pattern: pattern.to_string(),
            payload,
            reply: None,
            correlation_id: None,
        };
        let body = serde_json::to_vec(&wire)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
        let req_pc = self.partition(&self.options.requests_topic()).await?;
        let record = Record {
            key: None,
            value: Some(body),
            headers: BTreeMap::new(),
            timestamp: Utc::now(),
        };
        req_pc
            .produce(vec![record], Compression::default())
            .await
            .map_err(|e| TransportError::new(format!("kafka produce failed: {e}")))?;
        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_kafka_produce_total", "topic" => "requests")
            .increment(1);
        Ok(())
    }
}

/// Where the request-topic consumer starts on boot.
///
/// rskafka 0.6 has **no consumer-group / offset-commit API**, so the listener
/// keeps its position **in memory only**: every restart starts fresh at the
/// offset chosen here (and per-connection liveness is `at-most-once`).
///
/// - [`Latest`](Self::Latest) (default): skip everything already on the topic
///   and process only requests produced after boot. Restarts do **not**
///   re-execute old RPCs/events — the safe default: replaying a retained
///   backlog (broker default retention can be days) re-runs every handler,
///   including `Send` RPCs whose callers have long timed out.
/// - [`Earliest`](Self::Earliest): drain the full retained backlog on boot.
///   Intentional replay — every retained request is re-dispatched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KafkaConsumerStart {
    /// Start at the retained beginning — replay the whole backlog.
    Earliest,
    /// Start at the live tail — skip retained history (default).
    #[default]
    Latest,
}

impl From<KafkaConsumerStart> for OffsetAt {
    fn from(start: KafkaConsumerStart) -> Self {
        match start {
            KafkaConsumerStart::Earliest => OffsetAt::Earliest,
            KafkaConsumerStart::Latest => OffsetAt::Latest,
        }
    }
}

/// Server bootstrap options (topic layout matches [`KafkaTransportOptions`]).
#[derive(Clone, Debug)]
pub struct KafkaMicroserviceOptions {
    pub bootstrap_brokers: Vec<String>,
    pub topic_prefix: String,
    pub replication_factor: i16,
    pub create_topics: bool,
    pub connection: KafkaConnectionOptions,
    /// Consumer start on the requests topic (default [`KafkaConsumerStart::Latest`]).
    /// See [`KafkaConsumerStart`] for the replay trade-off.
    pub consumer_start: KafkaConsumerStart,
}

impl Default for KafkaMicroserviceOptions {
    fn default() -> Self {
        Self {
            bootstrap_brokers: vec!["127.0.0.1:9092".to_string()],
            topic_prefix: "nestrs".to_string(),
            replication_factor: 1i16,
            create_topics: true,
            connection: KafkaConnectionOptions::default(),
            consumer_start: KafkaConsumerStart::default(),
        }
    }
}

impl KafkaMicroserviceOptions {
    pub fn new(brokers: Vec<String>) -> Self {
        Self {
            bootstrap_brokers: brokers,
            ..Default::default()
        }
    }

    fn requests_topic(&self) -> String {
        format!("{}.requests", self.topic_prefix)
    }
}

/// Consumes `*.requests` partition 0 and dispatches `WireRequest` payloads (same as Redis micro listener).
pub struct KafkaMicroserviceServer {
    options: KafkaMicroserviceOptions,
    client: Mutex<Option<Arc<rskafka::client::Client>>>,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
    next_offset: Mutex<i64>,
}

impl KafkaMicroserviceServer {
    pub fn new(
        options: KafkaMicroserviceOptions,
        handlers: Vec<Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        Self {
            options,
            client: Mutex::new(None),
            handlers,
            next_offset: Mutex::new(0),
        }
    }

    async fn ensure_client(&self) -> Result<Arc<rskafka::client::Client>, TransportError> {
        let mut g = self.client.lock().await;
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
        let builder = client_builder_from_parts(
            self.options.bootstrap_brokers.clone(),
            &self.options.connection,
        )
        .map_err(|e| TransportError::new(format!("kafka client options: {e}")))?;
        let c = Arc::new(
            builder
                .build()
                .await
                .map_err(|e| TransportError::new(format!("kafka connect failed: {e}")))?,
        );
        if self.options.create_topics {
            if let Ok(ctrl) = c.controller_client() {
                let _ = ctrl
                    .create_topic(
                        self.options.requests_topic(),
                        1,
                        self.options.replication_factor,
                        5_000,
                    )
                    .await;
            }
        }
        let req_pc = c
            .partition_client(
                self.options.requests_topic(),
                0,
                UnknownTopicHandling::Retry,
            )
            .await
            .map_err(|e| TransportError::new(format!("kafka partition client failed: {e}")))?;
        // Boot position per `consumer_start` (Latest by default — no backlog
        // replay across restarts; see KafkaConsumerStart). A failed offset
        // fetch is surfaced instead of silently rewinding to 0 (which would
        // replay the whole topic).
        let start = req_pc
            .get_offset(self.options.consumer_start.into())
            .await
            .map_err(|e| {
                TransportError::new(format!("kafka get_offset (requests) failed: {e}"))
            })?;
        *self.next_offset.lock().await = start;
        *g = Some(c.clone());
        Ok(c)
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let client = self.ensure_client().await?;
        let requests_topic = self.options.requests_topic();
        let handlers = Arc::new(self.handlers);
        let next_offset = self.next_offset;
        let c2 = client;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {
                    let req_pc = match c2
                        .partition_client(requests_topic.clone(), 0, UnknownTopicHandling::Retry)
                        .await
                    {
                        Ok(p) => p,
                        Err(_) => continue,
                    };
                    let fetched = {
                        let off = next_offset.lock().await;
                        req_pc.fetch_records(*off, 1..4_000_000, 900).await
                    };
                    let (records, _) = match fetched {
                        Ok(x) => x,
                        Err(_) => continue,
                    };
                    if records.is_empty() {
                        continue;
                    }
                    let last_off = records.iter().map(|r| r.offset).max().unwrap_or(0);
                    {
                        let mut off = next_offset.lock().await;
                        *off = last_off + 1;
                    }
                    for ro in records {
                        let payload_bytes = match ro.record.value.as_deref() {
                            Some(b) => b,
                            None => continue,
                        };
                        let req: WireRequest = match serde_json::from_slice(payload_bytes) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        let handlers = handlers.clone();
                        let client = c2.clone();
                        match req.kind {
                            WireKind::Send => {
                                let Some(reply_topic) = req.reply.clone() else { continue };
                                let corr = req.correlation_id.clone().unwrap_or_default();
                                tokio::spawn(async move {
                                    let res = dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                    let reply_corr = req.correlation_id.clone();
                                    let wire = match res {
                                        Ok(v) => WireResponse {
                                            ok: true,
                                            payload: Some(v),
                                            error: None,
                                            correlation_id: reply_corr,
                                        },
                                        Err(e) => WireResponse {
                                            ok: false,
                                            payload: None,
                                            error: Some(WireError {
                                                message: e.message,
                                                details: e.details,
                                            }),
                                            correlation_id: req.correlation_id.clone(),
                                        },
                                    };
                                    if let Ok(bytes) = serde_json::to_vec(&wire) {
                                        if let Ok(rep_pc) = client
                                            .partition_client(reply_topic.clone(), 0, UnknownTopicHandling::Retry)
                                            .await
                                        {
                                            let rec = Record {
                                                key: Some(corr.into_bytes()),
                                                value: Some(bytes),
                                                headers: BTreeMap::new(),
                                                timestamp: Utc::now(),
                                            };
                                            let _ = rep_pc.produce(vec![rec], Compression::default()).await;
                                            #[cfg(feature = "microservice-metrics")]
                                            metrics::counter!("nestrs_microservice_kafka_produce_total", "topic" => "replies")
                                                .increment(1);
                                        }
                                    }
                                });
                            }
                            WireKind::Emit => {
                                let handlers = handlers.clone();
                                tokio::spawn(async move {
                                    dispatch_emit(&handlers, &req.pattern, req.payload.clone()).await;
                                });
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl MicroserviceServer for KafkaMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}

/// Liveness probe: broker accepts a Kafka connection (no topic I/O).
pub async fn kafka_cluster_reachable(brokers: Vec<String>) -> Result<(), TransportError> {
    kafka_cluster_reachable_with(brokers, &KafkaConnectionOptions::default()).await
}

/// Same as [`kafka_cluster_reachable`] but with TLS / SASL / `client.id`.
pub async fn kafka_cluster_reachable_with(
    brokers: Vec<String>,
    connection: &KafkaConnectionOptions,
) -> Result<(), TransportError> {
    let builder = client_builder_from_parts(brokers, connection)
        .map_err(|e| TransportError::new(format!("kafka client options: {e}")))?;
    builder
        .build()
        .await
        .map(|_| ())
        .map_err(|e| TransportError::new(format!("kafka broker unreachable: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_start_defaults_to_latest_no_replay() {
        // The audit-critical invariant: a fresh boot (and therefore every
        // restart — rskafka 0.6 has no offset-commit API, so position is
        // in-memory only) must NOT rewind to the retained beginning and
        // re-execute old RPCs/events.
        assert_eq!(KafkaConsumerStart::default(), KafkaConsumerStart::Latest);
        let options = KafkaMicroserviceOptions::default();
        assert_eq!(options.consumer_start, KafkaConsumerStart::Latest);
    }

    #[test]
    fn consumer_start_maps_to_rskafka_offset_at() {
        assert_eq!(
            OffsetAt::from(KafkaConsumerStart::Earliest),
            OffsetAt::Earliest
        );
        assert_eq!(OffsetAt::from(KafkaConsumerStart::Latest), OffsetAt::Latest);
    }
}
