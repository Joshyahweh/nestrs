use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct NatsTransportOptions {
    pub url: String,
    pub prefix: Option<String>,
    pub request_timeout: std::time::Duration,
}

impl std::fmt::Debug for NatsTransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // NATS URLs can embed `user:pass@` — redact the userinfo.
        f.debug_struct("NatsTransportOptions")
            .field("url", &crate::redact_url(&self.url))
            .field("prefix", &self.prefix)
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}

impl NatsTransportOptions {
    /// Subject namespace used when no explicit prefix is configured. Subscribing a
    /// bare `>` (the previous behavior) would pull in *every* subject on the cluster,
    /// dispatching unrelated traffic as RPC events.
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
        match self.prefix.as_deref().map(|p| p.trim().trim_matches('.')) {
            Some("") | None => Self::DEFAULT_PREFIX,
            Some(p) => p,
        }
    }

    fn subject(&self, pattern: &str) -> String {
        format!("{}.{pattern}", self.effective_prefix())
    }

    fn strip_prefix<'a>(&self, subject: &'a str) -> &'a str {
        let prefix_dot = format!("{}.", self.effective_prefix());
        subject.strip_prefix(&prefix_dot).unwrap_or(subject)
    }

    fn wildcard_subject(&self) -> String {
        // The listener wildcard must be its own dot-delimited token: `{prefix}.>`
        // matches every subject below the namespace. (An earlier version
        // emitted `{prefix}>` — a single literal token — which NATS treats as
        // an ordinary subject name, so the listener never received anything
        // published to `{prefix}.<pattern>`.)
        format!("{}.>", self.effective_prefix())
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
    // Boxed to mirror `TransportError::details` (serializes identically).
    details: Option<Box<serde_json::Value>>,
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

#[derive(Clone)]
pub struct NatsMicroserviceOptions {
    pub url: String,
    pub prefix: Option<String>,
    /// NATS queue group for the listener subscription. When set, multiple
    /// server instances subscribing in the same group each receive a message
    /// at most once (broker-side load balancing); without a group every
    /// instance receives every message, duplicating event side effects and
    /// racing RPC replies.
    pub queue_group: Option<String>,
}

impl std::fmt::Debug for NatsMicroserviceOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsMicroserviceOptions")
            .field("url", &crate::redact_url(&self.url))
            .field("prefix", &self.prefix)
            .field("queue_group", &self.queue_group)
            .finish()
    }
}

impl NatsMicroserviceOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            prefix: None,
            queue_group: None,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Subscribe the listener under a NATS queue group so that horizontally
    /// scaled instances load-share messages instead of each processing
    /// every one.
    pub fn with_queue_group(mut self, group: impl Into<String>) -> Self {
        self.queue_group = Some(group.into());
        self
    }
}

pub struct NatsMicroserviceServer {
    options: NatsTransportOptions,
    queue_group: Option<String>,
    handlers: Vec<std::sync::Arc<dyn MicroserviceHandler>>,
}

impl NatsMicroserviceServer {
    pub fn new(
        options: NatsMicroserviceOptions,
        handlers: Vec<std::sync::Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        let queue_group = options.queue_group;
        let options = NatsTransportOptions {
            url: options.url,
            prefix: options.prefix,
            request_timeout: std::time::Duration::from_secs(5),
        };
        Self {
            options,
            queue_group,
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
        let client = async_nats::connect(&self.options.url)
            .await
            .map_err(|e| TransportError::new(format!("nats microservice connect failed: {e}")))?;

        let subject = self.options.wildcard_subject();
        let mut sub = match self.queue_group.as_deref() {
            Some(group) => client
                .queue_subscribe(subject, group.to_string())
                .await
                .map_err(|e| TransportError::new(format!("nats queue subscribe failed: {e}")))?,
            None => client
                .subscribe(subject)
                .await
                .map_err(|e| TransportError::new(format!("nats subscribe failed: {e}")))?,
        };

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
                        Err(e) => {
                            // Unparseable payloads are dropped (NATS has no
                            // ack/redelivery to poison-loop), but not silently.
                            tracing::warn!(
                                subject = %msg.subject,
                                error = %e,
                                len = msg.payload.len(),
                                "nats: dropping malformed request"
                            );
                            continue;
                        }
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
                                if let Err(e) = client.publish(reply, bytes.into()).await {
                                    // The RPC caller has no signal except its
                                    // timeout; leave a server-side trace.
                                    tracing::warn!(
                                        error = %e,
                                        "nats reply publish failed; caller will time out"
                                    );
                                }
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

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn nats_url_credentials_never_reach_debug() {
        let opts = NatsTransportOptions::new("nats://daniel:hunter2-DO-NOT-LOG@nats.local:4222");
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("hunter2"), "password leaked: {rendered}");
        assert!(rendered.contains("nats://***@nats.local:4222"));

        let opts = NatsMicroserviceOptions::new("nats://user:pass@nats.local");
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("pass@"), "password leaked: {rendered}");
    }
}

#[cfg(test)]
mod subject_tests {
    use super::*;

    #[test]
    fn wildcard_subject_is_its_own_dot_delimited_token() {
        let opts = NatsTransportOptions::new("nats://localhost:4222");
        // `nestrs.>` (two tokens) matches every subject below the `nestrs`
        // namespace; `nestrs>` is one literal token that matches nothing the
        // transport ever publishes to.
        assert_eq!(opts.wildcard_subject(), "nestrs.>");
        assert_eq!(opts.subject("user.get"), "nestrs.user.get");
        assert_eq!(opts.strip_prefix("nestrs.user.get"), "user.get");

        let opts = opts.with_prefix("app");
        assert_eq!(opts.wildcard_subject(), "app.>");
    }

    #[test]
    fn queue_group_builder_round_trips() {
        let opts = NatsMicroserviceOptions::new("nats://localhost:4222");
        assert_eq!(opts.queue_group, None);
        let opts = opts.with_queue_group("payments");
        assert_eq!(opts.queue_group.as_deref(), Some("payments"));
        // Group names are not credentials; Debug keeps them for operators.
        let rendered = format!("{opts:?}");
        assert!(rendered.contains("payments"));
    }
}
