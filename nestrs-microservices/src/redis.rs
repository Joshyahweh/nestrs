use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

#[derive(Clone)]
pub struct RedisTransportOptions {
    pub url: String,
    pub prefix: Option<String>,
    pub request_timeout: std::time::Duration,
}

impl std::fmt::Debug for RedisTransportOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Redis URLs embed `:password@` / `user:pass@` — redact the userinfo.
        f.debug_struct("RedisTransportOptions")
            .field("url", &crate::redact_url(&self.url))
            .field("prefix", &self.prefix)
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
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

/// A command for the task that owns the transport's single long-lived pubsub
/// connection. Reply channels are single-use (one UUID each), so a channel is
/// either waiting for its one message or already released.
enum PubsubPumpCmd {
    /// Activate the reply `channel`, deliver its first message to `waiter`,
    /// and acknowledge once Redis confirms the subscription is live (the
    /// caller must not PUBLISH before that confirmation, or the reply can be
    /// missed).
    Subscribe {
        channel: String,
        waiter: oneshot::Sender<redis::Msg>,
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// Release a reply subscription. No-op when the pump already dropped it
    /// (reply delivered) or it never became active.
    Unsubscribe { channel: String },
}

/// The transport's shared, lazily-created connections.
struct SharedConns {
    /// Reconnecting multiplexed command connection — carries every PUBLISH
    /// for [`Transport::send_json`] and [`Transport::emit_json`] (and is
    /// cheap to clone; all clones share one TCP connection).
    manager: redis::aio::ConnectionManager,
    /// Feeds the pump task that owns the dedicated pubsub connection used for
    /// reply subscriptions. The pump exiting (Redis restart, connection loss)
    /// closes this sender, which [`RedisTransport::shared_conns`] detects and
    /// rebuilds on the next call.
    pump: mpsc::Sender<PubsubPumpCmd>,
}

/// Owns one dedicated pubsub connection for the lifetime of a
/// [`RedisTransport`] (or until the connection dies), routing messages on
/// per-RPC reply channels to their waiters. One connection total instead of
/// one per RPC — the per-RPC variant cost a TCP connect + handshake + teardown
/// cycle on every call, which under load turns into a connection storm against
/// the Redis server.
async fn spawn_pubsub_pump(
    client: redis::Client,
) -> Result<mpsc::Sender<PubsubPumpCmd>, String> {
    let pubsub = client
        .get_async_pubsub()
        .await
        .map_err(|e| format!("redis pubsub failed: {e}"))?;
    // `split` hands out an owned sink (subscribe/unsubscribe commands) and an
    // owned message stream, so both select arms can drive the one connection
    // without borrowing each other.
    let (mut sink, mut stream) = pubsub.split();
    let (tx, mut rx) = mpsc::channel::<PubsubPumpCmd>(64);
    tokio::spawn(async move {
        let mut waiters: HashMap<String, oneshot::Sender<redis::Msg>> = HashMap::new();
        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(PubsubPumpCmd::Subscribe { channel, waiter, ack }) => {
                            match sink.subscribe(&channel).await {
                                Ok(()) => {
                                    waiters.insert(channel, waiter);
                                    let _ = ack.send(Ok(()));
                                }
                                Err(e) => {
                                    let _ = ack.send(Err(format!("redis subscribe failed: {e}")));
                                }
                            }
                        }
                        Some(PubsubPumpCmd::Unsubscribe { channel }) => {
                            // Skip the round trip when the pump already released
                            // the channel (reply delivered / request timed out).
                            if waiters.remove(&channel).is_some() {
                                if let Err(e) = sink.unsubscribe(&channel).await {
                                    tracing::warn!(
                                        target: "nestrs_microservices",
                                        "redis unsubscribe `{channel}` failed: {e}"
                                    );
                                }
                            }
                        }
                        None => return,
                    }
                }
                maybe = stream.next() => {
                    let Some(msg) = maybe else { return };
                    let channel = msg.get_channel_name().to_string();
                    if let Some(waiter) = waiters.remove(&channel) {
                        let _ = waiter.send(msg);
                        // Single-use reply channel: free the server-side
                        // subscription slot immediately instead of waiting for
                        // the caller's `Unsubscribe` to arrive behind it.
                        if let Err(e) = sink.unsubscribe(&channel).await {
                            tracing::warn!(
                                target: "nestrs_microservices",
                                "redis unsubscribe `{channel}` failed: {e}"
                            );
                        }
                    }
                    // No waiter: a late reply for an RPC that already timed
                    // out and released its subscription — drop it.
                }
            }
        }
    });
    Ok(tx)
}

#[derive(Clone)]
pub struct RedisTransport {
    options: RedisTransportOptions,
    // Opened eagerly in `new` but never panics; URL errors surface on first use.
    client: Result<redis::Client, String>,
    // Shared connections (command manager + pubsub pump), created lazily on
    // first use and rebuilt if the pump task dies (connection loss). All
    // clones of the transport share one pair of connections.
    shared: Arc<Mutex<Option<SharedConns>>>,
}

impl RedisTransport {
    pub fn new(options: RedisTransportOptions) -> Self {
        let opened = redis::Client::open(options.url.clone())
            .map_err(|e| format!("redis client open failed: {e}"));
        Self {
            options,
            client: opened,
            shared: Arc::new(Mutex::new(None)),
        }
    }

    fn client(&self) -> Result<&redis::Client, TransportError> {
        self.client
            .as_ref()
            .map_err(|msg| TransportError::new(msg.clone()))
    }

    /// The shared command manager + pubsub pump, connecting on first use.
    /// Holding the lock across the connects serializes first-use callers into
    /// a single connect (no thundering herd of managers); on failure the cell
    /// stays empty so the next call retries. The connect budget is
    /// `request_timeout` — a black-holed address must fail the RPC, not hang
    /// it while other callers queue on the lock.
    async fn shared_conns(
        &self,
    ) -> Result<(redis::aio::ConnectionManager, mpsc::Sender<PubsubPumpCmd>), TransportError>
    {
        let client = self.client()?.clone();
        let mut guard = self.shared.lock().await;
        if let Some(shared) = guard.as_ref() {
            if !shared.pump.is_closed() {
                return Ok((shared.manager.clone(), shared.pump.clone()));
            }
            // The pump task is gone (pubsub connection lost) — rebuild.
            *guard = None;
        }
        let budget = self.options.request_timeout;
        let manager = tokio::time::timeout(budget, client.get_connection_manager())
            .await
            .map_err(|_| TransportError::new("redis connect timed out"))?
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        let pump = tokio::time::timeout(budget, spawn_pubsub_pump(client))
            .await
            .map_err(|_| TransportError::new("redis pubsub connect timed out"))?
            .map_err(TransportError::new)?;
        *guard = Some(SharedConns {
            manager: manager.clone(),
            pump: pump.clone(),
        });
        Ok((manager, pump))
    }
}

#[async_trait]
impl Transport for RedisTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        // A fresh uuid per request makes the reply channel unguessable and collision-free across
        // processes (a shared atomic counter is not), and doubles as the correlation id.
        let correlation_id = Uuid::new_v4().simple().to_string();
        let reply = RedisTransportOptions::reply_channel(&correlation_id);
        let channel = self.options.channel(pattern);

        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply.clone()),
            correlation_id: Some(correlation_id.clone()),
        };
        let text = serde_json::to_string(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let (manager, pump) = self.shared_conns().await?;

        // Register the reply waiter and wait for Redis to confirm the
        // subscription — publishing before the confirmation races the reply
        // into a channel we are not listening on yet.
        let (waiter, waiter_rx) = oneshot::channel::<redis::Msg>();
        let (ack, ack_rx) = oneshot::channel();
        pump.send(PubsubPumpCmd::Subscribe {
            channel: reply.clone(),
            waiter,
            ack,
        })
        .await
        .map_err(|_| TransportError::new("redis pubsub connection unavailable"))?;
        ack_rx
            .await
            .map_err(|_| TransportError::new("redis pubsub connection unavailable"))?
            .map_err(TransportError::new)?;

        // Publish and await the reply under one whole-RPC budget, matching
        // the transport contract of the other adapters.
        let outcome = tokio::time::timeout(self.options.request_timeout, async {
            let mut conn = manager.clone();
            redis::cmd("PUBLISH")
                .arg(&channel)
                .arg(&text)
                .query_async::<i64>(&mut conn)
                .await
                .map_err(|e| TransportError::new(format!("redis publish failed: {e}")))?;
            let msg = waiter_rx
                .await
                .map_err(|_| TransportError::new("redis pubsub connection unavailable"))?;
            let payload: String = msg.get_payload().map_err(|e| {
                TransportError::new(format!("redis reply payload decode failed: {e}"))
            })?;
            let wire: WireResponse = serde_json::from_str(&payload)
                .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))?;
            Ok::<_, TransportError>(wire)
        })
        .await
        .map_err(|_| TransportError::new("redis request timed out"));

        // Release the single-use reply subscription on every path — success,
        // timeout, or error. A dead pump means the connection (and its
        // subscriptions) is already gone.
        if pump
            .send(PubsubPumpCmd::Unsubscribe { channel: reply })
            .await
            .is_err()
        {
            tracing::warn!(
                target: "nestrs_microservices",
                "redis pubsub pump unavailable; reply subscription released by connection loss"
            );
        }

        // `outcome` is Result<Result<WireResponse, TransportError>, TransportError>:
        // the outer `?` unwraps the RPC budget, the inner the RPC itself.
        let wire = outcome??;
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

        let (manager, _pump) = self.shared_conns().await?;
        let mut conn = manager.clone();
        tokio::time::timeout(self.options.request_timeout, async {
            redis::cmd("PUBLISH")
                .arg(&channel)
                .arg(&text)
                .query_async::<i64>(&mut conn)
                .await
        })
        .await
        .map_err(|_| TransportError::new("redis publish timed out"))?
        .map_err(|e| TransportError::new(format!("redis publish failed: {e}")))?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct RedisMicroserviceOptions {
    pub url: String,
    pub prefix: Option<String>,
}

impl std::fmt::Debug for RedisMicroserviceOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisMicroserviceOptions")
            .field("url", &crate::redact_url(&self.url))
            .field("prefix", &self.prefix)
            .finish()
    }
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
        // One reconnecting command connection for every reply the server will
        // ever publish — each handled request previously dialed a fresh
        // multiplexed connection, a per-RPC TCP connect/handshake/teardown
        // cycle. The manager self-heals if Redis restarts mid-listen.
        let client = self.client()?.clone();
        let manager = client
            .get_connection_manager()
            .await
            .map_err(|e| TransportError::new(format!("redis connect failed: {e}")))?;
        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| TransportError::new(format!("redis pubsub failed: {e}")))?;
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
                            let mut conn = manager.clone();
                            // Echo the caller's correlation id so it can reject stale replies.
                            let reply_corr = req.correlation_id.clone();
                            tokio::spawn(async move {
                                let res = dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                let wire = match res {
                                    Ok(v) => WireResponse { ok: true, payload: Some(v), error: None, correlation_id: reply_corr },
                                    Err(e) => WireResponse { ok: false, payload: None, error: Some(WireError { message: e.message, details: e.details }), correlation_id: reply_corr },
                                };
                                if let Ok(text) = serde_json::to_string(&wire) {
                                    let _ = redis::cmd("PUBLISH")
                                        .arg(&reply)
                                        .arg(text)
                                        .query_async::<i64>(&mut conn)
                                        .await;
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

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn redis_url_credentials_never_reach_debug() {
        let opts = RedisTransportOptions::new("redis://:hunter2-DO-NOT-LOG@cache.local:6379");
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("hunter2"), "password leaked: {rendered}");
        assert!(rendered.contains("redis://***@cache.local:6379"));

        let opts = RedisMicroserviceOptions::new("redis://svc:pencil1@cache.local");
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("pencil1"), "password leaked: {rendered}");
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use std::time::Duration;

    /// A refused port the transport can never reach. Binding and dropping a
    /// listener guarantees the port is closed rather than merely unlikely to
    /// be open.
    fn dead_url() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        format!("redis://127.0.0.1:{port}")
    }

    fn fast_opts(url: String) -> RedisTransportOptions {
        let mut opts = RedisTransportOptions::new(url);
        opts.request_timeout = Duration::from_millis(500);
        opts
    }

    #[tokio::test]
    async fn unreachable_redis_fails_the_rpc_without_hanging() {
        let transport = RedisTransport::new(fast_opts(dead_url()));
        let started = std::time::Instant::now();
        let err = transport
            .send_json("probe", serde_json::json!({"n": 1}))
            .await
            .expect_err("unreachable redis must fail the RPC");
        // Generous ceiling — the point is "errored", not "hung".
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "send_json hung for {:?}: {err:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn unreachable_redis_does_not_poison_the_transport() {
        // A failed connect must leave the shared-connection cell empty (not
        // wedged or cached as a failure) so the next call retries cleanly.
        let transport = RedisTransport::new(fast_opts(dead_url()));
        for _ in 0..2 {
            assert!(
                transport.send_json("probe", serde_json::json!({})).await.is_err(),
                "each call against unreachable redis must error"
            );
        }
        assert!(
            transport.emit_json("probe", serde_json::json!({})).await.is_err(),
            "emit against unreachable redis must error"
        );
    }
}
