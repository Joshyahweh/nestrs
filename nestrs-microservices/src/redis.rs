use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RedisTransportOptions {
    pub url: String,
    pub prefix: Option<String>,
    pub request_timeout: std::time::Duration,
}

impl RedisTransportOptions {
    /// Channel namespace used when no explicit prefix is configured. A bare `*` psubscribe
    /// (the previous behavior) would consume *every* pubsub message on the server, dispatching
    /// unrelated traffic as RPC events.
    const DEFAULT_PREFIX: &'static str = "nestrs";

    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            prefix: None,
            request_timeout: std::time::Duration::from_secs(5),
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    fn effective_prefix(&self) -> &str {
        match self
            .prefix
            .as_deref()
            .map(|p| p.trim().trim_end_matches('.'))
        {
            Some("") | None => Self::DEFAULT_PREFIX,
            Some(p) => p,
        }
    }

    fn channel(&self, pattern: &str) -> String {
        format!("{}.{pattern}", self.effective_prefix())
    }

    fn wildcard(&self) -> String {
        format!("{}.*", self.effective_prefix())
    }

    /// Per-request reply channel, kept *outside* the `{prefix}.*` namespace so servers
    /// subscribed to the wildcard never receive each other's replies.
    fn reply_channel(correlation_id: &str) -> String {
        format!("__nestrs.reply.{correlation_id}")
    }
}

#[derive(Clone)]
pub struct RedisTransport {
    options: RedisTransportOptions,
    // Opened eagerly in `new` but never panics; URL errors surface on first use.
    client: Result<redis::Client, String>,
}

impl RedisTransport {
    pub fn new(options: RedisTransportOptions) -> Self {
        let opened = redis::Client::open(options.url.clone())
            .map_err(|e| format!("redis client open failed: {e}"));
        Self {
            options,
            client: opened,
        }
    }

    fn client(&self) -> Result<&redis::Client, TransportError> {
        self.client
            .as_ref()
            .map_err(|msg| TransportError::new(msg.clone()))
    }
}

#[async_trait]
impl Transport for RedisTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        let client = self.client()?;
        // A fresh uuid per request makes the reply channel unguessable and collision-free across
        // processes (a shared atomic counter is not), and doubles as the correlation id.
        let correlation_id = Uuid::new_v4().simple().to_string();
        let reply = RedisTransportOptions::reply_channel(&correlation_id);
        let channel = self.options.channel(pattern);

        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| TransportError::new(format!("redis pubsub failed: {e}")))?;
        pubsub
            .subscribe(&reply)
            .await
            .map_err(|e| TransportError::new(format!("redis subscribe failed: {e}")))?;

        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply),
            correlation_id: Some(correlation_id.clone()),
        };
        let text = serde_json::to_string(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let mut pub_conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(text)
            .query_async::<i64>(&mut pub_conn)
            .await
            .map_err(|e| TransportError::new(format!("redis publish failed: {e}")))?;

        let mut stream = pubsub.on_message();
        let msg = tokio::time::timeout(self.options.request_timeout, stream.next())
            .await
            .map_err(|_| TransportError::new("redis request timed out"))?
            .ok_or_else(|| TransportError::new("redis request timed out"))?;

        let payload: String = msg
            .get_payload()
            .map_err(|e| TransportError::new(format!("redis reply payload decode failed: {e}")))?;
        let wire: WireResponse = serde_json::from_str(&payload)
            .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))?;
        // Reject stale/mismatched replies on a recycled channel. Absent id = legacy peer
        // (pre-correlation responder); accepted for wire compatibility (see `wire` module docs).
        if let Some(id) = &wire.correlation_id {
            if id != &correlation_id {
                return Err(TransportError::new(
                    "redis reply correlation mismatch (stale or forged response)",
                ));
            }
        }
        if wire.ok {
            Ok(wire.payload.unwrap_or(serde_json::Value::Null))
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

    async fn emit_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<(), TransportError> {
        let channel = self.options.channel(pattern);
        let wire = WireRequest {
            kind: WireKind::Emit,
            pattern: pattern.to_string(),
            payload,
            reply: None,
            correlation_id: None,
        };
        let text = serde_json::to_string(&wire)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;

        let mut conn = self
            .client()?
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(text)
            .query_async::<i64>(&mut conn)
            .await
            .map_err(|e| TransportError::new(format!("redis publish failed: {e}")))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RedisMicroserviceOptions {
    pub url: String,
    pub prefix: Option<String>,
}

impl RedisMicroserviceOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            prefix: None,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
}

pub struct RedisMicroserviceServer {
    options: RedisTransportOptions,
    // Opened eagerly in `new` but never panics; URL errors surface on `listen`.
    client: Result<redis::Client, String>,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
}

impl RedisMicroserviceServer {
    pub fn new(
        options: RedisMicroserviceOptions,
        handlers: Vec<Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        let options = RedisTransportOptions {
            url: options.url,
            prefix: options.prefix,
            request_timeout: std::time::Duration::from_secs(5),
        };
        let opened = redis::Client::open(options.url.clone())
            .map_err(|e| format!("redis client open failed: {e}"));
        Self {
            client: opened,
            options,
            handlers,
        }
    }

    fn client(&self) -> Result<&redis::Client, TransportError> {
        self.client
            .as_ref()
            .map_err(|msg| TransportError::new(msg.clone()))
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let mut pubsub = self
            .client()?
            .get_async_pubsub()
            .await
            .map_err(|e| TransportError::new(format!("redis pubsub failed: {e}")))?;
        pubsub
            .psubscribe(self.options.wildcard())
            .await
            .map_err(|e| TransportError::new(format!("redis psubscribe failed: {e}")))?;

        let handlers = Arc::new(self.handlers);
        // Clone the (possibly failed) open result out of `self` before it's partially moved.
        let opened_client = self.client.clone();
        let mut stream = pubsub.on_message();

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                maybe = stream.next() => {
                    let Some(msg) = maybe else { break; };
                    let payload: String = match msg.get_payload() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let req: WireRequest = match serde_json::from_str(&payload) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    match req.kind {
                        WireKind::Send => {
                            let Some(reply) = req.reply else { continue; };
                            let handlers = handlers.clone();
                            // A failed URL open surfaces as a skipped request here; it was
                            // already reported at `listen` time via `listen_with_shutdown`.
                            let Ok(client) = opened_client.as_ref() else { continue };
                            let client = client.clone();
                            // Echo the caller's correlation id so it can reject stale replies.
                            let reply_corr = req.correlation_id.clone();
                            tokio::spawn(async move {
                                let res = dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                let wire = match res {
                                    Ok(v) => WireResponse { ok: true, payload: Some(v), error: None, correlation_id: reply_corr },
                                    Err(e) => WireResponse { ok: false, payload: None, error: Some(WireError { message: e.message, details: e.details }), correlation_id: reply_corr },
                                };
                                if let Ok(text) = serde_json::to_string(&wire) {
                                    if let Ok(mut conn) = client.get_multiplexed_async_connection().await
                                    {
                                        let _ = redis::cmd("PUBLISH")
                                            .arg(&reply)
                                            .arg(text)
                                            .query_async::<i64>(&mut conn)
                                            .await;
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

        Ok(())
    }
}

#[async_trait]
impl crate::MicroserviceServer for RedisMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: crate::ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
