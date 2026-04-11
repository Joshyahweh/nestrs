use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Clone, Debug)]
pub struct NatsTransportOptions {
    pub url: String,
    pub prefix: Option<String>,
    pub request_timeout: std::time::Duration,
}

impl NatsTransportOptions {
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

    fn subject(&self, pattern: &str) -> String {
        match self.prefix.as_deref() {
            None => pattern.to_string(),
            Some(p) => {
                let p = p.trim_matches('.');
                if p.is_empty() {
                    pattern.to_string()
                } else {
                    format!("{p}.{pattern}")
                }
            }
        }
    }

    fn strip_prefix<'a>(&self, subject: &'a str) -> &'a str {
        let Some(p) = self.prefix.as_deref() else {
            return subject;
        };
        let p = p.trim_matches('.');
        if p.is_empty() {
            return subject;
        }
        let prefix_dot = format!("{p}.");
        subject.strip_prefix(&prefix_dot).unwrap_or(subject)
    }

    fn wildcard_subject(&self) -> String {
        match self.prefix.as_deref() {
            None => ">".to_string(),
            Some(p) => {
                let p = p.trim_matches('.');
                if p.is_empty() {
                    ">".to_string()
                } else {
                    format!("{p}.>")
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct NatsTransport {
    options: NatsTransportOptions,
    client: OnceCell<async_nats::Client>,
}

impl NatsTransport {
    pub fn new(options: NatsTransportOptions) -> Self {
        Self {
            options,
            client: OnceCell::new(),
        }
    }

    async fn client(&self) -> Result<&async_nats::Client, TransportError> {
        self.client
            .get_or_try_init(|| async {
                async_nats::connect(&self.options.url)
                    .await
                    .map_err(|e| TransportError::new(format!("nats connect failed: {e}")))
            })
            .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireError {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WireError>,
}

#[async_trait]
impl Transport for NatsTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        let client = self.client().await?;
        let subject = self.options.subject(pattern);
        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let msg = tokio::time::timeout(
            self.options.request_timeout,
            client.request(subject, bytes.into()),
        )
        .await
        .map_err(|_| TransportError::new("nats request timed out"))?
        .map_err(|e| TransportError::new(format!("nats request failed: {e}")))?;

        let wire: WireResponse = serde_json::from_slice(&msg.payload)
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
        let client = self.client().await?;
        let subject = self.options.subject(pattern);
        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
        client
            .publish(subject, bytes.into())
            .await
            .map_err(|e| TransportError::new(format!("nats publish failed: {e}")))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct NatsMicroserviceOptions {
    pub url: String,
    pub prefix: Option<String>,
}

impl NatsMicroserviceOptions {
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

pub struct NatsMicroserviceServer {
    options: NatsTransportOptions,
    handlers: Vec<std::sync::Arc<dyn MicroserviceHandler>>,
}

impl NatsMicroserviceServer {
    pub fn new(
        options: NatsMicroserviceOptions,
        handlers: Vec<std::sync::Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        let options = NatsTransportOptions {
            url: options.url,
            prefix: options.prefix,
            request_timeout: std::time::Duration::from_secs(5),
        };
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
        let client = async_nats::connect(&self.options.url)
            .await
            .map_err(|e| TransportError::new(format!("nats microservice connect failed: {e}")))?;

        let mut sub = client
            .subscribe(self.options.wildcard_subject())
            .await
            .map_err(|e| TransportError::new(format!("nats subscribe failed: {e}")))?;

        let handlers = std::sync::Arc::new(self.handlers);

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                maybe = sub.next() => {
                    let Some(msg) = maybe else { break; };
                    let pattern = self.options.strip_prefix(&msg.subject).to_string();
                    let payload: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let handlers = handlers.clone();
                    let client = client.clone();
                    let reply = msg.reply.clone();
                    tokio::spawn(async move {
                        if let Some(reply) = reply {
                            let res = dispatch_send(&handlers, &pattern, payload).await;
                            let wire = match res {
                                Ok(v) => WireResponse { ok: true, payload: Some(v), error: None },
                                Err(e) => WireResponse {
                                    ok: false,
                                    payload: None,
                                    error: Some(WireError { message: e.message, details: e.details }),
                                },
                            };
                            if let Ok(bytes) = serde_json::to_vec(&wire) {
                                let _ = client.publish(reply, bytes.into()).await;
                            }
                        } else {
                            dispatch_emit(&handlers, &pattern, payload).await;
                        }
                    });
                }
            }
        }

        Ok(())
    }
}

async fn dispatch_send(
    handlers: &[std::sync::Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, TransportError> {
    for h in handlers {
        if let Some(res) = h.handle_message(pattern, payload.clone()).await {
            return res;
        }
    }
    Err(TransportError::new(format!(
        "no microservice handler for pattern `{pattern}`"
    )))
}

async fn dispatch_emit(
    handlers: &[std::sync::Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: serde_json::Value,
) {
    for h in handlers {
        let _ = h.handle_event(pattern, payload.clone()).await;
    }
}

#[async_trait]
impl crate::MicroserviceServer for NatsMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: crate::ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
