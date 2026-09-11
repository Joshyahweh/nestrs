//! Optional microservices transport primitives for nestrs (Phase 4 roadmap crate).
//!
//! This crate intentionally starts with a tiny, stable interface so transports (NATS/Redis/gRPC)
//! can be added incrementally without blocking core HTTP framework progress.
//!
//! ## Cross-cutting on message handlers
//!
//! On `#[micro_routes]` impl blocks, per-handler attributes **`#[use_micro_interceptors(...)]`**,
//! **`#[use_micro_guards(...)]`**, and **`#[use_micro_pipes(...)]`** run before your
//! `#[message_pattern]` / `#[event_pattern]` body (order: interceptors → guards → pipes). This is
//! the closest analogue to Nest’s microservice pipes/guards/interceptors; there is no separate
//! exception-filter pipeline — return [`TransportError`] from handlers (the `nestrs` crate maps
//! `HttpException` into [`TransportError`] with JSON details in generated `#[micro_routes]` code).

// lapin's auto-trait chains (`pinky_swear` → `flume` → `lock_api`) exceed
// rustc's default trait-solver recursion depth when `RabbitMqTransport:
// Sync` is evaluated (newer nightlies promote this via the
// `recursion-depth-exceeding-limit` lint). Raise the crate limit instead of
// restructuring the transport; no behavior change.
#![recursion_limit = "256"]

pub mod custom;
pub mod wire;

pub use wire::WIRE_FORMAT_DOC_REVISION;

/// Render a connection URL with any embedded `user:pass@` userinfo removed.
///
/// AMQP, Redis, and NATS URLs commonly carry credentials in the authority
/// (`amqp://user:pass@host/vhost`, `redis://:password@host`). A derived
/// `Debug` prints the raw string into logs, panic messages, and error
/// reports — this helper keeps the scheme/host visible for operators while
/// never rendering the credential bytes.
// Callers live behind the optional nats/redis/rabbitmq transport features;
// without any of them this is legitimately dead in the lib target (the
// `#[cfg(test)]` redaction tests below still exercise it in every config).
#[cfg_attr(
    not(any(feature = "nats", feature = "redis", feature = "rabbitmq")),
    allow(dead_code)
)]
pub(crate) fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let auth_start = scheme_end + 3;
    let Some(at) = url[auth_start..].find('@') else {
        return url.to_string();
    };
    let at = auth_start + at;
    // Only redact userinfo: an '@' after the first '/', '?' or '#' is part
    // of a path/query (e.g. `amqp://host/vhost@odd`), not a credential.
    let auth = &url[auth_start..at];
    if auth.contains('/') || auth.contains('?') || auth.contains('#') {
        return url.to_string();
    }
    format!("{}***@{}", &url[..auth_start], &url[at + 1..])
}

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "grpc")]
mod grpc;
mod kafka;
mod mqtt;
#[cfg(feature = "nats")]
mod nats;
mod rabbitmq;
#[cfg(feature = "redis")]
mod redis;
mod tcp;

#[cfg(feature = "grpc")]
pub use grpc::{
    GrpcMicroserviceOptions, GrpcMicroserviceServer, GrpcTransport, GrpcTransportOptions,
};
pub use kafka::KafkaTransport;
#[cfg(feature = "kafka")]
pub use kafka::{
    kafka_cluster_reachable, kafka_cluster_reachable_with, KafkaConnectionOptions,
    KafkaConsumerStart, KafkaMicroserviceOptions, KafkaMicroserviceServer, KafkaSaslOptions,
    KafkaTlsOptions, KafkaTransportOptions,
};
pub use mqtt::MqttTransport;
#[cfg(feature = "mqtt")]
pub use mqtt::{
    MqttMicroserviceOptions, MqttMicroserviceServer, MqttSocketOptions, MqttTlsMode,
    MqttTransportOptions,
};
#[cfg(feature = "nats")]
pub use nats::{
    NatsMicroserviceOptions, NatsMicroserviceServer, NatsTransport, NatsTransportOptions,
};
pub use nestrs_events::EventBus;
pub use rabbitmq::RabbitMqTransport;
#[cfg(feature = "rabbitmq")]
pub use rabbitmq::{
    RabbitMqMicroserviceOptions, RabbitMqMicroserviceServer, RabbitMqTransportOptions,
};
#[cfg(feature = "redis")]
pub use redis::{
    RedisMicroserviceOptions, RedisMicroserviceServer, RedisTransport, RedisTransportOptions,
};
pub use tcp::{
    TcpMicroserviceOptions, TcpMicroserviceServer, TcpTransport, TcpTransportOptions,
    MAX_FRAME_BYTES,
};

#[doc(hidden)]
pub use linkme;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope<T> {
    pub pattern: String,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    // Boxed to mirror `HttpException::details`, keeping error types small
    // (serializes identically over the wire).
    pub details: Option<Box<serde_json::Value>>,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<Box<serde_json::Value>>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Authorization / policy hook before a microservice handler runs (Nest microservice guard analogue).
#[async_trait]
pub trait MicroCanActivate: Default + Send + Sync + 'static {
    /// Build the guard instance for a message. The default returns
    /// `Self::default()` (stateless guards); override it to pull
    /// dependencies from the registry, mirroring HTTP guard DI
    /// (`CanActivate::resolve`). The registry-aware dispatch generated by
    /// `#[micro_routes]` and installed by `#[module(microservices = [...])]`
    /// calls this **per message** (request-scoped semantics) — a guard that
    /// only implements `Default` keeps working unchanged.
    fn resolve(_registry: &nestrs_core::ProviderRegistry) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    async fn can_activate_micro(
        &self,
        pattern: &str,
        payload: &serde_json::Value,
    ) -> Result<(), TransportError>;
}

/// Transform inbound JSON after guards (Nest microservice pipe analogue).
#[async_trait]
pub trait MicroPipeTransform: Default + Send + Sync + 'static {
    /// Build the pipe instance for a message; see [`MicroCanActivate::resolve`].
    fn resolve(_registry: &nestrs_core::ProviderRegistry) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    async fn transform_micro(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError>;
}

/// Observe inbound patterns (logging / metrics); does not fail the pipeline.
#[async_trait]
pub trait MicroIncomingInterceptor: Default + Send + Sync + 'static {
    /// Build the interceptor instance for a message; see
    /// [`MicroCanActivate::resolve`].
    fn resolve(_registry: &nestrs_core::ProviderRegistry) -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    async fn before_handle_micro(&self, pattern: &str, payload: &serde_json::Value);
}

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError>;
    async fn emit_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<(), TransportError>;
}

/// A Nest-style microservice handler registry entrypoint (controller/service methods annotated with
/// `#[message_pattern]` / `#[event_pattern]` via the `#[micro_routes]` impl macro).
#[async_trait]
pub trait MicroserviceHandler: Send + Sync + 'static {
    /// Handle a request/reply message pattern. Return `None` when the handler doesn't match `pattern`.
    /// Declared patterns may use NATS-style wildcards (`*`, `>`); see [`pattern_matches`].
    async fn handle_message(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Option<Result<serde_json::Value, TransportError>>;

    /// Handle a fire-and-forget event pattern. Return `true` when the handler matched `pattern`.
    /// Declared patterns may use NATS-style wildcards (`*`, `>`); see [`pattern_matches`].
    async fn handle_event(&self, pattern: &str, payload: serde_json::Value) -> bool;
}

/// Registry-aware dispatch, generated by `#[micro_routes]` next to the plain
/// [`MicroserviceHandler`] impl.
///
/// Same dispatch, but `#[use_micro_guards]` / `#[use_micro_pipes]` /
/// `#[use_micro_interceptors]` instances are built through
/// [`MicroCanActivate::resolve`] / [`MicroPipeTransform::resolve`] /
/// [`MicroIncomingInterceptor::resolve`] with the app's registry — so guards
/// with injected dependencies receive them instead of a silently-empty
/// `Default`. `handler_factory` wraps every handler from
/// `#[module(microservices = [...])]` in [`__NestrsMicroserviceRegistryHandler`],
/// which routes the transport servers' plain
/// [`MicroserviceHandler::handle_message`] calls through this trait without
/// any server-side signature changes.
///
/// The plain [`MicroserviceHandler`] impl keeps `Default` construction and
/// remains valid for dispatching an `Arc<T>` you resolved yourself (outside
/// the module factory).
#[async_trait]
pub trait RegistryAwareMicroserviceHandler: MicroserviceHandler {
    /// Registry-aware counterpart of [`MicroserviceHandler::handle_message`].
    async fn handle_message_with_registry(
        &self,
        pattern: &str,
        payload: serde_json::Value,
        registry: &nestrs_core::ProviderRegistry,
    ) -> Option<Result<serde_json::Value, TransportError>>;

    /// Registry-aware counterpart of [`MicroserviceHandler::handle_event`].
    async fn handle_event_with_registry(
        &self,
        pattern: &str,
        payload: serde_json::Value,
        registry: &nestrs_core::ProviderRegistry,
    ) -> bool;
}

/// `#[doc(hidden)]` wrapper installed by [`handler_factory`]: carries the
/// app's registry next to the DI-resolved handler so transport servers —
/// which dispatch plain [`MicroserviceHandler`] — transparently reach the
/// registry-aware methods generated by `#[micro_routes]`.
#[doc(hidden)]
pub struct __NestrsMicroserviceRegistryHandler<T> {
    inner: std::sync::Arc<T>,
    registry: nestrs_core::ProviderRegistry,
}

#[async_trait]
impl<T: RegistryAwareMicroserviceHandler> MicroserviceHandler
    for __NestrsMicroserviceRegistryHandler<T>
{
    async fn handle_message(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Option<Result<serde_json::Value, TransportError>> {
        self.inner
            .handle_message_with_registry(pattern, payload, &self.registry)
            .await
    }

    async fn handle_event(&self, pattern: &str, payload: serde_json::Value) -> bool {
        self.inner
            .handle_event_with_registry(pattern, payload, &self.registry)
            .await
    }
}

/// NATS-style wildcard matching for microservice handler patterns.
///
/// `#[message_pattern]` / `#[event_pattern]` handlers declare patterns that a
/// transport matches against the concrete pattern it delivers (for NATS, the
/// subject with the namespace prefix stripped). A declared pattern may use
/// NATS subject wildcards: `*` matches exactly one dot-delimited token, and a
/// trailing `>` matches one or more trailing tokens — so `user.*` matches
/// `user.get` but not `user.profile.get` or `user`, and `audit.>` matches
/// `audit.created` and `audit.user.deleted` but not `audit`. A non-final `>`
/// is compared literally (NATS rejects it in subscriptions; a subject
/// containing `>` may still be published).
///
/// Every transport delivers at least the concrete pattern, so wildcard
/// handler patterns behave the same across transports: the NATS listener
/// subscribes `{prefix}.>` and the Redis listener `{prefix}.*` (Redis's `*`
/// is a glob that spans dots), while TCP/MQTT/RabbitMQ/Kafka carry the
/// pattern inside the request payload itself.
pub fn pattern_matches(declared: &str, incoming: &str) -> bool {
    let declared_tokens: Vec<&str> = declared.split('.').collect();
    let incoming_tokens: Vec<&str> = incoming.split('.').collect();
    let mut d = 0;
    let mut i = 0;
    while d < declared_tokens.len() {
        let token = declared_tokens[d];
        // Trailing `>` matches one or more remaining tokens.
        if token == ">" && d == declared_tokens.len() - 1 {
            return i < incoming_tokens.len();
        }
        if i >= incoming_tokens.len() {
            return false;
        }
        if token != "*" && token != incoming_tokens[i] {
            return false;
        }
        d += 1;
        i += 1;
    }
    i == incoming_tokens.len()
}

#[cfg(test)]
mod pattern_matches_tests {
    use super::pattern_matches;

    #[test]
    fn literal_patterns_match_exactly() {
        assert!(pattern_matches("user.get", "user.get"));
        assert!(!pattern_matches("user.get", "user.getx"));
        assert!(!pattern_matches("user.get", "user.profile.get"));
        assert!(!pattern_matches("user.get", "user"));
    }

    #[test]
    fn star_matches_exactly_one_token() {
        assert!(pattern_matches("user.*", "user.get"));
        assert!(pattern_matches("*.created", "user.created"));
        assert!(pattern_matches("a.*.c", "a.b.c"));
        assert!(!pattern_matches("user.*", "user.profile.get"));
        assert!(!pattern_matches("user.*", "user"));
        assert!(!pattern_matches("user.*.created", "user.created"));
    }

    #[test]
    fn trailing_gt_matches_one_or_more_tokens() {
        assert!(pattern_matches("audit.>", "audit.created"));
        assert!(pattern_matches("audit.>", "audit.user.deleted"));
        assert!(!pattern_matches("audit.>", "audit"));
        assert!(pattern_matches(">", "anything"));
        assert!(pattern_matches(">", "a.b.c"));
    }

    #[test]
    fn non_trailing_gt_is_compared_literally() {
        assert!(pattern_matches("a.>.b", "a.>.b"));
        assert!(!pattern_matches("a.>.b", "a.x.b"));
    }

    #[test]
    fn token_counts_must_line_up() {
        assert!(!pattern_matches("a.b", "a.b.c"));
        assert!(!pattern_matches("a.b.c", "a.b"));
        assert!(pattern_matches("*", "a"));
        assert!(!pattern_matches("*", "a.b"));
    }

    #[test]
    fn mixed_wildcards_compose() {
        assert!(pattern_matches("*.user.>", "app.user.created.now"));
        assert!(!pattern_matches("*.user.>", "user.created"));
        assert!(pattern_matches("a.*.>", "a.b.c"));
    }
}

pub type MicroserviceHandlerFactory =
    fn(&nestrs_core::ProviderRegistry) -> Arc<dyn MicroserviceHandler>;

pub type ShutdownFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[async_trait]
pub trait MicroserviceServer: Send + Sync + 'static {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: ShutdownFuture,
    ) -> Result<(), TransportError>;
}

/// Implemented by `#[module(microservices = [...])]` to declare which providers handle patterns.
pub trait MicroserviceModule {
    fn microservice_handlers() -> Vec<MicroserviceHandlerFactory>;
}

pub fn handler_factory<T>(registry: &nestrs_core::ProviderRegistry) -> Arc<dyn MicroserviceHandler>
where
    T: nestrs_core::Injectable + RegistryAwareMicroserviceHandler,
{
    // Wrap so `#[use_micro_guards]` / `#[use_micro_pipes]` /
    // `#[use_micro_interceptors]` resolve through the app's registry
    // (DI-backed guards) instead of `Default::default()`.
    Arc::new(__NestrsMicroserviceRegistryHandler {
        inner: registry.get::<T>(),
        registry: registry.clone(),
    })
}

/// Nest-like client proxy wrapper over a configured [`Transport`].
#[derive(Clone)]
pub struct ClientProxy {
    transport: Arc<dyn Transport>,
}

impl ClientProxy {
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    pub async fn send<TReq, TRes>(
        &self,
        pattern: &str,
        payload: &TReq,
    ) -> Result<TRes, TransportError>
    where
        TReq: Serialize + Send + Sync,
        TRes: for<'de> Deserialize<'de> + Send,
    {
        let req = serde_json::to_value(payload)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;
        let res = self.transport.send_json(pattern, req).await?;
        serde_json::from_value(res)
            .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))
    }

    pub async fn emit<TReq>(&self, pattern: &str, payload: &TReq) -> Result<(), TransportError>
    where
        TReq: Serialize + Send + Sync,
    {
        let req = serde_json::to_value(payload)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
        self.transport.emit_json(pattern, req).await
    }
}

#[async_trait]
impl nestrs_core::Injectable for ClientProxy {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!(
            "ClientProxy must be provided by ClientsModule::register(...) or constructed manually"
        );
    }
}

/// Auto-wiring registration entry for `#[event_routes]` + `#[on_event("...")]` handlers.
pub struct OnEventRegistration {
    pub register: fn(&nestrs_core::ProviderRegistry),
}

#[linkme::distributed_slice]
pub static ON_EVENT_REGISTRATIONS: [OnEventRegistration] = [..];

/// Subscribe all `#[on_event]` handlers registered via `#[event_routes]`.
pub fn wire_on_event_handlers(registry: &nestrs_core::ProviderRegistry) {
    for reg in ON_EVENT_REGISTRATIONS.iter() {
        (reg.register)(registry);
    }
}

#[derive(Clone)]
pub struct ClientConfig {
    pub name: &'static str,
    pub transport: Arc<dyn Transport>,
}

impl ClientConfig {
    pub fn new(name: &'static str, transport: Arc<dyn Transport>) -> Self {
        Self { name, transport }
    }

    pub fn tcp(name: &'static str, options: TcpTransportOptions) -> Self {
        Self::new(name, Arc::new(TcpTransport::new(options)))
    }

    #[cfg(feature = "nats")]
    pub fn nats(name: &'static str, options: NatsTransportOptions) -> Self {
        Self::new(name, Arc::new(NatsTransport::new(options)))
    }

    #[cfg(feature = "redis")]
    pub fn redis(name: &'static str, options: RedisTransportOptions) -> Self {
        Self::new(name, Arc::new(RedisTransport::new(options)))
    }

    #[cfg(feature = "grpc")]
    pub fn grpc(name: &'static str, options: GrpcTransportOptions) -> Self {
        Self::new(name, Arc::new(GrpcTransport::new(options)))
    }

    #[cfg(feature = "kafka")]
    pub fn kafka(name: &'static str, options: KafkaTransportOptions) -> Self {
        Self::new(name, Arc::new(KafkaTransport::new(options)))
    }

    #[cfg(not(feature = "kafka"))]
    pub fn kafka(name: &'static str) -> Self {
        Self::new(name, Arc::new(KafkaTransport::new()))
    }

    #[cfg(feature = "mqtt")]
    pub fn mqtt(name: &'static str, options: MqttTransportOptions) -> Self {
        Self::new(name, Arc::new(MqttTransport::new(options)))
    }

    #[cfg(not(feature = "mqtt"))]
    pub fn mqtt(name: &'static str) -> Self {
        Self::new(name, Arc::new(MqttTransport::new()))
    }

    #[cfg(feature = "rabbitmq")]
    pub fn rabbitmq(name: &'static str, options: RabbitMqTransportOptions) -> Self {
        Self::new(name, Arc::new(RabbitMqTransport::new(options)))
    }

    #[cfg(not(feature = "rabbitmq"))]
    pub fn rabbitmq(name: &'static str) -> Self {
        Self::new(name, Arc::new(RabbitMqTransport::new()))
    }
}

#[derive(Clone)]
pub struct ClientsService {
    clients: Arc<HashMap<&'static str, ClientProxy>>,
}

impl ClientsService {
    pub fn get(&self, name: &str) -> Option<ClientProxy> {
        self.clients.get(name).cloned()
    }

    pub fn expect(&self, name: &str) -> ClientProxy {
        self.get(name).unwrap_or_else(|| {
            let known = self.clients.keys().copied().collect::<Vec<_>>().join(", ");
            panic!("ClientProxy `{name}` not registered. Known clients: [{known}]");
        })
    }
}

#[async_trait]
impl nestrs_core::Injectable for ClientsService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("ClientsService must be provided by ClientsModule::register(...)");
    }
}

pub struct ClientsModule;

impl ClientsModule {
    /// Register named microservice clients into a runtime [`nestrs_core::DynamicModule`].
    ///
    /// Exports:
    /// - [`ClientsService`]
    /// - [`EventBus`]
    /// - [`ClientProxy`] **only** when exactly one client config is provided (default client).
    pub fn register(configs: &[ClientConfig]) -> nestrs_core::DynamicModule {
        if configs.is_empty() {
            panic!("ClientsModule::register requires at least one ClientConfig");
        }

        let mut seen = std::collections::HashSet::<&'static str>::new();
        let mut clients = HashMap::<&'static str, ClientProxy>::new();
        for cfg in configs {
            if !seen.insert(cfg.name) {
                panic!(
                    "ClientsModule::register: duplicate client name `{}`",
                    cfg.name
                );
            }
            clients.insert(cfg.name, ClientProxy::new(cfg.transport.clone()));
        }

        let mut registry = nestrs_core::ProviderRegistry::new();
        registry.register::<EventBus>();

        let clients_service = Arc::new(ClientsService {
            clients: Arc::new(clients),
        });
        registry.override_provider::<ClientsService>(clients_service);

        let mut exports = vec![TypeId::of::<ClientsService>(), TypeId::of::<EventBus>()];

        if configs.len() == 1 {
            let first = &configs[0];
            registry.override_provider::<ClientProxy>(Arc::new(ClientProxy::new(
                first.transport.clone(),
            )));
            exports.push(TypeId::of::<ClientProxy>());
        }

        nestrs_core::DynamicModule::from_parts(registry, axum::Router::new(), exports)
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn redact_url_strips_userinfo_credentials() {
        // user:pass
        assert_eq!(
            redact_url("amqp://guest:hunter2@rabbit.local:5672/vhost"),
            "amqp://***@rabbit.local:5672/vhost"
        );
        // password-only (Redis convention: empty username)
        assert_eq!(
            redact_url("redis://:s3cr3t@cache.local:6379/0"),
            "redis://***@cache.local:6379/0"
        );
        // username only — the userinfo is still redacted whole
        assert_eq!(redact_url("nats://daniel@nats.local"), "nats://***@nats.local");
        // percent-encoded '@' inside the password is left intact in the
        // authority (it is not a raw '@'), so the first '@' is the separator.
        assert_eq!(
            redact_url("amqp://u:p%40ss@host"),
            "amqp://***@host"
        );
    }

    #[test]
    fn redact_url_leaves_credential_free_urls_alone() {
        assert_eq!(redact_url("amqp://rabbit.local/vhost"), "amqp://rabbit.local/vhost");
        assert_eq!(redact_url("redis://cache.local:6379"), "redis://cache.local:6379");
        // An '@' after a path separator is not userinfo.
        assert_eq!(
            redact_url("amqp://host/vhost@example"),
            "amqp://host/vhost@example"
        );
        assert_eq!(redact_url("not a url"), "not a url");
    }
}
