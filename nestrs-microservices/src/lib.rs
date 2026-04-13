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

pub mod custom;
pub mod wire;

pub use wire::WIRE_FORMAT_DOC_REVISION;

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
    KafkaMicroserviceOptions, KafkaMicroserviceServer, KafkaSaslOptions, KafkaTlsOptions,
    KafkaTransportOptions,
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
pub use tcp::{TcpMicroserviceOptions, TcpMicroserviceServer, TcpTransport, TcpTransportOptions};

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
    pub details: Option<serde_json::Value>,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Authorization / policy hook before a microservice handler runs (Nest microservice guard analogue).
#[async_trait]
pub trait MicroCanActivate: Default + Send + Sync + 'static {
    async fn can_activate_micro(
        &self,
        pattern: &str,
        payload: &serde_json::Value,
    ) -> Result<(), TransportError>;
}

/// Transform inbound JSON after guards (Nest microservice pipe analogue).
#[async_trait]
pub trait MicroPipeTransform: Default + Send + Sync + 'static {
    async fn transform_micro(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError>;
}

/// Observe inbound patterns (logging / metrics); does not fail the pipeline.
#[async_trait]
pub trait MicroIncomingInterceptor: Default + Send + Sync + 'static {
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
    async fn handle_message(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Option<Result<serde_json::Value, TransportError>>;

    /// Handle a fire-and-forget event pattern. Return `true` when the handler matched `pattern`.
    async fn handle_event(&self, pattern: &str, payload: serde_json::Value) -> bool;
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
    T: nestrs_core::Injectable + MicroserviceHandler,
{
    registry.get::<T>()
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
