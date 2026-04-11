//! Production MQTT transport using [rumqttc](https://docs.rs/rumqttc) (native TLS stack).
//!
//! Uses the same JSON [`crate::wire::WireRequest`] as Redis/Kafka on a single requests topic; RPC replies are
//! published to a per-request reply topic carried in `WireRequest.reply`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rumqttc::{
    AsyncClient, Event, Incoming, MqttOptions, QoS, TlsConfiguration, Transport as RumqttTransport,
};
use serde_json::Value;
use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, MicroserviceServer, ShutdownFuture, Transport, TransportError};

/// TLS mode for MQTT (native-tls).
#[derive(Clone, Debug)]
pub enum MqttTlsMode {
    /// OS trust store (e.g. `mqtts` to public brokers).
    Native,
    /// PEM-encoded CA certificate bytes.
    CaPem(Vec<u8>),
}

/// Broker socket security (TLS + username/password).
#[derive(Clone, Debug, Default)]
pub struct MqttSocketOptions {
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: Option<MqttTlsMode>,
}

/// Client options.
#[derive(Clone, Debug)]
pub struct MqttTransportOptions {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub request_timeout: Duration,
    pub topic_prefix: String,
    pub socket: MqttSocketOptions,
}

impl Default for MqttTransportOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 1883,
            client_id: format!("nestrs-{}", Uuid::new_v4().simple()),
            request_timeout: Duration::from_secs(30),
            topic_prefix: "nestrs".to_string(),
            socket: MqttSocketOptions::default(),
        }
    }
}

fn apply_mqtt_socket(mqtt_opts: &mut MqttOptions, socket: &MqttSocketOptions) {
    if let Some(tls) = &socket.tls {
        let transport = match tls {
            MqttTlsMode::Native => RumqttTransport::tls_with_config(TlsConfiguration::Native),
            MqttTlsMode::CaPem(ca) => {
                RumqttTransport::tls_with_config(TlsConfiguration::SimpleNative {
                    ca: ca.clone(),
                    client_auth: None,
                })
            }
        };
        mqtt_opts.set_transport(transport);
    }
    if let (Some(u), Some(p)) = (&socket.username, &socket.password) {
        mqtt_opts.set_credentials(u.clone(), p.clone());
    }
}

impl MqttTransportOptions {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    fn requests_topic(&self) -> String {
        let p = self.topic_prefix.trim_matches('/');
        format!("{p}/rpc/requests")
    }
}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>;

/// Nest-style MQTT [`Transport`].
pub struct MqttTransport {
    options: MqttTransportOptions,
    client: AsyncClient,
    pending: PendingMap,
}

impl MqttTransport {
    pub fn new(options: MqttTransportOptions) -> Self {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let mut mqtt_opts = MqttOptions::new(&options.client_id, &options.host, options.port);
        mqtt_opts.set_keep_alive(Duration::from_secs(30));
        apply_mqtt_socket(&mut mqtt_opts, &options.socket);

        let (client, mut eventloop) = AsyncClient::new(mqtt_opts, 64);
        let pend = pending.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        let t = p.topic.clone();
                        let mut map = pend.lock().await;
                        if let Some(tx) = map.remove(&t) {
                            let s = String::from_utf8_lossy(p.payload.as_ref()).to_string();
                            let _ = tx.send(s);
                        }
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
        });

        Self {
            options,
            client,
            pending,
        }
    }

    async fn wait_reply(&self, rx: oneshot::Receiver<String>) -> Result<String, TransportError> {
        tokio::time::timeout(self.options.request_timeout, rx)
            .await
            .map_err(|_| TransportError::new("mqtt request timed out"))?
            .map_err(|_| TransportError::new("mqtt reply channel closed"))
    }
}

#[async_trait]
impl Transport for MqttTransport {
    async fn send_json(&self, pattern: &str, payload: Value) -> Result<Value, TransportError> {
        let reply_topic = format!(
            "{}/rpc/reply/{}",
            self.options.topic_prefix.trim_matches('/'),
            Uuid::new_v4().simple()
        );
        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply_topic.clone()),
            correlation_id: None,
        };
        let body = serde_json::to_vec(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        self.client
            .subscribe(&reply_topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| TransportError::new(format!("mqtt subscribe failed: {e}")))?;

        let (tx, rx) = oneshot::channel();
        {
            let mut map = self.pending.lock().await;
            map.insert(reply_topic.clone(), tx);
        }

        let req_topic = self.options.requests_topic();
        self.client
            .publish(&req_topic, QoS::AtLeastOnce, false, body)
            .await
            .map_err(|e| TransportError::new(format!("mqtt publish failed: {e}")))?;
        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_mqtt_publish_total", "topic" => "requests")
            .increment(1);

        let text = match self.wait_reply(rx).await {
            Ok(t) => t,
            Err(e) => {
                self.pending.lock().await.remove(&reply_topic);
                return Err(e);
            }
        };
        self.pending.lock().await.remove(&reply_topic);

        let wire: WireResponse = serde_json::from_str(&text)
            .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))?;
        if wire.ok {
            Ok(wire.payload.unwrap_or(Value::Null))
        } else {
            let err = wire.error.unwrap_or(WireError {
                message: "microservice error".to_string(),
                details: None,
            });
            let mut out = TransportError::new(err.message);
            if let Some(details) = err.details {
                out = out.with_details(details);
            }
            Err(out)
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
        let req_topic = self.options.requests_topic();
        self.client
            .publish(&req_topic, QoS::AtLeastOnce, false, body)
            .await
            .map_err(|e| TransportError::new(format!("mqtt publish failed: {e}")))?;
        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_mqtt_publish_total", "topic" => "requests")
            .increment(1);
        Ok(())
    }
}

/// MQTT microservice listener options.
#[derive(Clone, Debug)]
pub struct MqttMicroserviceOptions {
    pub host: String,
    pub port: u16,
    pub client_id: String,
    pub topic_prefix: String,
    pub socket: MqttSocketOptions,
}

impl Default for MqttMicroserviceOptions {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 1883,
            client_id: format!("nestrs-ms-{}", Uuid::new_v4().simple()),
            topic_prefix: "nestrs".to_string(),
            socket: MqttSocketOptions::default(),
        }
    }
}

impl MqttMicroserviceOptions {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    fn requests_topic(&self) -> String {
        let p = self.topic_prefix.trim_matches('/');
        format!("{p}/rpc/requests")
    }
}

/// Subscribes to `{prefix}/rpc/requests` and dispatches [`WireRequest`] payloads.
pub struct MqttMicroserviceServer {
    options: MqttMicroserviceOptions,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
}

impl MqttMicroserviceServer {
    pub fn new(
        options: MqttMicroserviceOptions,
        handlers: Vec<Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        Self { options, handlers }
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut mqtt_opts = MqttOptions::new(
            &self.options.client_id,
            &self.options.host,
            self.options.port,
        );
        mqtt_opts.set_keep_alive(Duration::from_secs(30));
        apply_mqtt_socket(&mut mqtt_opts, &self.options.socket);
        let (client, mut eventloop) = AsyncClient::new(mqtt_opts, 128);
        let topic = self.options.requests_topic();
        let handlers = Arc::new(self.handlers);

        client
            .subscribe(&topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| TransportError::new(format!("mqtt subscribe failed: {e}")))?;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                ev = eventloop.poll() => {
                    match ev {
                        Ok(Event::Incoming(Incoming::Publish(p))) => {
                            let payload_bytes = p.payload.as_ref();
                            let req: WireRequest = match serde_json::from_slice(payload_bytes) {
                                Ok(v) => v,
                                Err(_) => continue,
                            };
                            let handlers = handlers.clone();
                            let client = client.clone();
                            match req.kind {
                                WireKind::Send => {
                                    let Some(reply_topic) = req.reply.clone() else { continue };
                                    tokio::spawn(async move {
                                        let res = dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                        let wire = match res {
                                            Ok(v) => WireResponse {
                                                ok: true,
                                                payload: Some(v),
                                                error: None,
                                            },
                                            Err(e) => WireResponse {
                                                ok: false,
                                                payload: None,
                                                error: Some(WireError {
                                                    message: e.message,
                                                    details: e.details,
                                                }),
                                            },
                                        };
                                        if let Ok(bytes) = serde_json::to_vec(&wire) {
                                            let _ = client
                                                .publish(&reply_topic, QoS::AtLeastOnce, false, bytes)
                                                .await;
                                            #[cfg(feature = "microservice-metrics")]
                                            metrics::counter!("nestrs_microservice_mqtt_publish_total", "topic" => "replies")
                                                .increment(1);
                                        }
                                    });
                                }
                                WireKind::Emit => {
                                    tokio::spawn(async move {
                                        dispatch_emit(&handlers, &req.pattern, req.payload.clone()).await;
                                    });
                                }
                            }
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl MicroserviceServer for MqttMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
