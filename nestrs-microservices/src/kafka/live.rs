//! Production Kafka transport using [rskafka](https://docs.rs/rskafka) (pure Rust).
//!
//! Wire format matches Redis/NATS: JSON `WireRequest` payloads on the `requests` topic; replies go to
//! a per-client `replies.{instance_id}` topic with record key = `correlation_id`.

use std::collections::{BTreeMap, HashMap};
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
    /// One partition client per topic, kept for the life of the transport.
    ///
    /// Constructing an rskafka `PartitionClient` forces a full leader
    /// discovery (multiple metadata round-trips); rebuilding per RPC
    /// repeated that on every call. A constructed client self-heals — it
    /// migrates transparently across leader changes and broken connections —
    /// so caching it for the transport's lifetime is safe.
    partitions: Mutex<HashMap<String, Arc<rskafka::client::partition::PartitionClient>>>,
}

impl KafkaTransport {
    pub fn new(options: KafkaTransportOptions) -> Self {
        Self {
            instance_id: Uuid::new_v4().simple().to_string(),
            options,
            client: Mutex::new(None),
            partitions: Mutex::new(HashMap::new()),
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
        let create_topics = self.options.create_topics;
        let requests_topic = self.options.requests_topic();
        let replies_topic = self.options.replies_topic(&self.instance_id);
        let replication_factor = self.options.replication_factor;
        // rskafka retries broker connects / metadata internally with no
        // deadline; bound the whole bootstrap so `request_timeout` still
        // means something against a black-holed address.
        let c = tokio::time::timeout(self.options.request_timeout, async {
            let c = builder
                .build()
                .await
                .map_err(|e| TransportError::new(format!("kafka connect failed: {e}")))?;
            if create_topics {
                if let Ok(ctrl) = c.controller_client() {
                    let _ = ctrl
                        .create_topic(requests_topic, 1, replication_factor, 5_000)
                        .await;
                    let _ = ctrl
                        .create_topic(replies_topic, 1, replication_factor, 5_000)
                        .await;
                }
            }
            Ok::<_, TransportError>(c)
        })
        .await
        .map_err(|_| TransportError::new("kafka connect timed out"))??;
        let c = Arc::new(c);
        *g = Some(c.clone());
        Ok(c)
    }

    async fn partition(
        &self,
        topic: &str,
    ) -> Result<Arc<rskafka::client::partition::PartitionClient>, TransportError> {
        // Connect before taking the partition-cache lock so the two locks
        // are never nested.
        let c = self.connect().await?;
        cached_partition(&c, &self.partitions, topic, self.options.request_timeout).await
    }
}

/// Partition client for `topic` (partition 0 — single-partition topic
/// layout), reusing `cache`. The cache lock is held across the
/// (timeout-bounded) construction so concurrent callers don't each pay
/// leader discovery (single-flight); cache hits return immediately.
async fn cached_partition(
    client: &rskafka::client::Client,
    cache: &Mutex<HashMap<String, Arc<rskafka::client::partition::PartitionClient>>>,
    topic: &str,
    timeout: std::time::Duration,
) -> Result<Arc<rskafka::client::partition::PartitionClient>, TransportError> {
    let mut g = cache.lock().await;
    if let Some(pc) = g.get(topic) {
        return Ok(Arc::clone(pc));
    }
    let pc = tokio::time::timeout(
        timeout,
        client.partition_client(topic.to_owned(), 0, UnknownTopicHandling::Retry),
    )
    .await
    .map_err(|_| TransportError::new(format!("kafka partition client `{topic}` timed out")))?
    .map_err(|e| TransportError::new(format!("kafka partition client `{topic}` failed: {e}")))?;
    let pc = Arc::new(pc);
    g.insert(topic.to_owned(), Arc::clone(&pc));
    Ok(pc)
}

/// Produce `record` on `pc`, bounded by `cap` — rskafka retries internally
/// with no deadline, so an unreachable broker would otherwise park the
/// caller forever.
async fn bounded_produce(
    pc: &rskafka::client::partition::PartitionClient,
    record: Record,
    cap: std::time::Duration,
) -> Result<(), TransportError> {
    tokio::time::timeout(cap, pc.produce(vec![record], Compression::default()))
        .await
        .map_err(|_| TransportError::new("kafka produce timed out"))?
        .map(|_| ())
        .map_err(|e| TransportError::new(format!("kafka produce failed: {e}")))
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

        let start_off = tokio::time::timeout(
            self.options.request_timeout,
            rep_pc.get_offset(OffsetAt::Latest),
        )
        .await
        .map_err(|_| TransportError::new("kafka get_offset (replies) timed out"))?
        .map_err(|e| TransportError::new(format!("kafka get_offset (replies) failed: {e}")))?;

        let record = Record {
            key: None,
            value: Some(body),
            headers: BTreeMap::new(),
            timestamp: Utc::now(),
        };
        bounded_produce(&req_pc, record, self.options.request_timeout).await?;
        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_kafka_produce_total", "topic" => "requests")
            .increment(1);

        let deadline = tokio::time::Instant::now() + self.options.request_timeout;
        let mut next_off = start_off;

        loop {
            // Bound each fetch by the remaining budget: rskafka retries
            // internally with no deadline, so without this a broker dying
            // mid-RPC hangs `send_json` far past `request_timeout` (the
            // deadline check below only ran *between* fetches).
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let fetched =
                tokio::time::timeout(remaining, rep_pc.fetch_records(next_off, 1..1_000_000, 500))
                    .await
                    .map_err(|_| TransportError::new("kafka request timed out"))?;
            let (records, _) = fetched
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
        bounded_produce(&req_pc, record, self.options.request_timeout).await?;
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
            .map_err(|e| TransportError::new(format!("kafka get_offset (requests) failed: {e}")))?;
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

        tokio::pin!(shutdown);

        // One partition client for the whole listener (was: rebuilt every
        // 25 ms poll tick — a full leader discovery, i.e. multiple metadata
        // round-trips, per tick, even idle). Cancellable against shutdown so
        // a broker that is down at boot doesn't wedge `listen`.
        let req_pc = loop {
            tokio::select! {
                _ = &mut shutdown => return Ok(()),
                built = client.partition_client(requests_topic.clone(), 0, UnknownTopicHandling::Retry) => match built {
                    Ok(p) => break Arc::new(p),
                    Err(e) => {
                        tracing::warn!(
                            topic = %requests_topic,
                            error = %e,
                            "kafka requests partition unavailable; retrying in 1s"
                        );
                        tokio::select! {
                            _ = &mut shutdown => return Ok(()),
                            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                        }
                    }
                }
            }
        };

        // Reply partition clients, shared by every reply task (was: one
        // fresh partition client — leader discovery and all — per reply).
        let reply_pcs: Arc<
            Mutex<HashMap<String, Arc<rskafka::client::partition::PartitionClient>>>,
        > = Arc::new(Mutex::new(HashMap::new()));

        // In-memory consumer position (rskafka 0.6 has no offset-commit
        // API); advanced before dispatch (at-most-once), same as before.
        let mut next_off = *self.next_offset.lock().await;

        // Pacing/backoff for the poll loop. Empty fetches are normal
        // idleness (the 900 ms broker-side long-poll paces them); errors
        // and stalls back off exponentially instead of hot-retrying a
        // dead broker every 25 ms.
        let mut error_backoff_ms: u64 = 250;
        const ERROR_BACKOFF_MAX_MS: u64 = 10_000;
        const POLL_STALL_CAP: std::time::Duration = std::time::Duration::from_secs(30);
        const REPLY_PRODUCE_CAP: std::time::Duration = std::time::Duration::from_secs(30);

        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                fetched = tokio::time::timeout(
                    POLL_STALL_CAP,
                    req_pc.fetch_records(next_off, 1..4_000_000, 900),
                ) => {
                    match fetched {
                        // Fetch stuck >30s: the broker went unreachable
                        // mid-poll (rskafka retries internally with no
                        // deadline). Warn and start a fresh cycle; the
                        // internal backoff keeps pacing within each cycle.
                        Err(_stalled) => {
                            tracing::warn!(
                                topic = %requests_topic,
                                "kafka requests poll stalled for 30s (broker unreachable?); restarting poll cycle"
                            );
                        }
                        Ok(Err(e)) => {
                            tracing::warn!(
                                topic = %requests_topic,
                                error = %e,
                                backoff_ms = error_backoff_ms,
                                "kafka requests fetch failed; backing off"
                            );
                            tokio::select! {
                                _ = &mut shutdown => break,
                                _ = tokio::time::sleep(std::time::Duration::from_millis(error_backoff_ms)) => {}
                            }
                            error_backoff_ms = (error_backoff_ms * 2).min(ERROR_BACKOFF_MAX_MS);
                        }
                        Ok(Ok((records, _))) if records.is_empty() => {
                            // Idle: the 900 ms long-poll paced this cycle;
                            // the short floor only guards brokers that
                            // return empty instantly. No backoff growth —
                            // idleness is normal.
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        }
                        Ok(Ok((records, _))) => {
                            error_backoff_ms = 250;
                            let last_off = records.iter().map(|r| r.offset).max().unwrap_or(next_off);
                            next_off = last_off + 1;
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
                                let reply_pcs = reply_pcs.clone();
                                let client = client.clone();
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
                                                let rep_pc = match cached_partition(
                                                    &client,
                                                    &reply_pcs,
                                                    &reply_topic,
                                                    REPLY_PRODUCE_CAP,
                                                )
                                                .await
                                                {
                                                    Ok(pc) => pc,
                                                    Err(e) => {
                                                        tracing::warn!(
                                                            error = %e.message,
                                                            "kafka reply partition unavailable; dropping reply"
                                                        );
                                                        return;
                                                    }
                                                };
                                                let rec = Record {
                                                    key: Some(corr.into_bytes()),
                                                    value: Some(bytes),
                                                    headers: BTreeMap::new(),
                                                    timestamp: Utc::now(),
                                                };
                                                if let Err(e) = bounded_produce(&rep_pc, rec, REPLY_PRODUCE_CAP).await {
                                                    tracing::warn!(
                                                        error = %e.message,
                                                        "kafka reply produce failed; caller will time out"
                                                    );
                                                    return;
                                                }
                                                #[cfg(feature = "microservice-metrics")]
                                                metrics::counter!("nestrs_microservice_kafka_produce_total", "topic" => "replies")
                                                    .increment(1);
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
    // The probe must fail fast: rskafka retries broker connects internally
    // with no deadline, and a health check that hangs forever wedges the
    // prober.
    tokio::time::timeout(std::time::Duration::from_secs(10), builder.build())
        .await
        .map_err(|_| TransportError::new("kafka broker unreachable: probe timed out"))?
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

#[cfg(test)]
mod connection_tests {
    use super::*;
    use std::time::Duration;

    /// Binds + drops a listener so the port is guaranteed-refused (a
    /// hard-coded port could be taken by a real broker).
    fn dead_addr() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind temp listener");
        let addr = l.local_addr().expect("local addr");
        drop(l);
        format!("127.0.0.1:{}", addr.port())
    }

    fn dead_transport(request_timeout: Duration) -> KafkaTransport {
        let mut opts = KafkaTransportOptions::new(vec![dead_addr()]);
        opts.request_timeout = request_timeout;
        opts.create_topics = false;
        KafkaTransport::new(opts)
    }

    #[tokio::test]
    async fn unreachable_kafka_fails_the_rpc_without_hanging() {
        let transport = dead_transport(Duration::from_millis(500));
        let start = std::time::Instant::now();
        let err = transport
            .send_json("audit.ping", serde_json::json!({"n": 1}))
            .await
            .expect_err("unreachable broker must fail the RPC");
        // `request_timeout` must be honored end-to-end (rskafka retries
        // internally with no deadline; without the transport-level bounds
        // this call hangs forever).
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "send_json hung: {:?} ({err:?})",
            start.elapsed()
        );
    }

    #[tokio::test]
    async fn unreachable_kafka_fails_the_rpc_again_after_a_failure() {
        // A failed connect must leave the shared client cell empty so the
        // next call retries (is not poisoned).
        let transport = dead_transport(Duration::from_millis(500));
        transport
            .send_json("audit.ping", serde_json::json!({}))
            .await
            .expect_err("first call must fail");
        let err = transport
            .send_json("audit.ping", serde_json::json!({}))
            .await
            .expect_err("second call must fail too");
        assert!(err.message.contains("kafka"), "unexpected error: {err:?}");
    }

    #[tokio::test]
    async fn unreachable_kafka_fails_emit_without_hanging() {
        let transport = dead_transport(Duration::from_millis(500));
        let start = std::time::Instant::now();
        transport
            .emit_json("audit.tick", serde_json::json!({"seq": 1}))
            .await
            .expect_err("unreachable broker must fail the emit");
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "emit_json hung: {:?}",
            start.elapsed()
        );
    }
}
