use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct RedisTransportOptions {
    pub url: String,
    pub prefix: Option<String>,
    pub request_timeout: std::time::Duration,
}

impl RedisTransportOptions {
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

    fn channel(&self, pattern: &str) -> String {
        match self.prefix.as_deref() {
            None => pattern.to_string(),
            Some(p) => {
                let p = p.trim_end_matches('.');
                if p.is_empty() {
                    pattern.to_string()
                } else {
                    format!("{p}.{pattern}")
                }
            }
        }
    }

    fn wildcard(&self) -> String {
        match self.prefix.as_deref() {
            None => "*".to_string(),
            Some(p) => {
                let p = p.trim_end_matches('.');
                if p.is_empty() {
                    "*".to_string()
                } else {
                    format!("{p}.*")
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RedisTransport {
    options: RedisTransportOptions,
    client: redis::Client,
    seq: Arc<AtomicU64>,
}

impl RedisTransport {
    pub fn new(options: RedisTransportOptions) -> Self {
        let client = redis::Client::open(options.url.clone())
            .unwrap_or_else(|e| panic!("redis client open failed: {e}"));
        Self {
            options,
            client,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    fn next_id(&self) -> String {
        self.seq.fetch_add(1, Ordering::Relaxed).to_string()
    }
}

#[async_trait]
impl Transport for RedisTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        let id = self.next_id();
        let reply = format!("__nestrs.reply.{id}");
        let channel = self.options.channel(pattern);

        let mut conn = self
            .client
            .get_async_connection()
            .await
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        let mut pubsub = conn.into_pubsub();
        pubsub
            .subscribe(&reply)
            .await
            .map_err(|e| TransportError::new(format!("redis subscribe failed: {e}")))?;

        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply.clone()),
            correlation_id: None,
        };
        let text = serde_json::to_string(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let mut pub_conn = self
            .client
            .get_async_connection()
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
            .client
            .get_async_connection()
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
    client: redis::Client,
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
        let client = redis::Client::open(options.url.clone())
            .unwrap_or_else(|e| panic!("redis client open failed: {e}"));
        Self {
            options,
            client,
            handlers,
        }
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let conn = self
            .client
            .get_async_connection()
            .await
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        let mut pubsub = conn.into_pubsub();
        pubsub
            .psubscribe(self.options.wildcard())
            .await
            .map_err(|e| TransportError::new(format!("redis psubscribe failed: {e}")))?;

        let handlers = Arc::new(self.handlers);
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
                            let client = self.client.clone();
                            tokio::spawn(async move {
                                let res = dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                let wire = match res {
                                    Ok(v) => WireResponse { ok: true, payload: Some(v), error: None },
                                    Err(e) => WireResponse { ok: false, payload: None, error: Some(WireError { message: e.message, details: e.details }) },
                                };
                                if let Ok(text) = serde_json::to_string(&wire) {
                                    if let Ok(mut conn) = client.get_async_connection().await {
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
