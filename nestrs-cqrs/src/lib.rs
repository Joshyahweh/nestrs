use async_trait::async_trait;
use nestrs_core::{Injectable, Module, ModuleGraph, ProviderRegistry};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CqrsError {
    pub message: String,
}

impl CqrsError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CqrsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CqrsError {}

pub trait Command: Send + Sync + 'static {
    type Response: Send + Sync + 'static;
}

pub trait Query: Send + Sync + 'static {
    type Response: Send + Sync + 'static;
}

#[async_trait]
pub trait CommandHandler<C>: Send + Sync + 'static
where
    C: Command,
{
    async fn execute(&self, command: C) -> Result<C::Response, CqrsError>;
}

#[async_trait]
pub trait QueryHandler<Q>: Send + Sync + 'static
where
    Q: Query,
{
    async fn execute(&self, query: Q) -> Result<Q::Response, CqrsError>;
}

#[async_trait]
trait ErasedCommandHandler: Send + Sync + 'static {
    fn command_type_id(&self) -> TypeId;
    async fn execute_boxed(&self, command: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, CqrsError>;
}

struct CommandHandlerBox<H, C>
where
    H: CommandHandler<C>,
    C: Command,
{
    inner: Arc<H>,
    _marker: std::marker::PhantomData<fn() -> C>,
}

#[async_trait]
impl<H, C> ErasedCommandHandler for CommandHandlerBox<H, C>
where
    H: CommandHandler<C>,
    C: Command,
{
    fn command_type_id(&self) -> TypeId {
        TypeId::of::<C>()
    }

    async fn execute_boxed(
        &self,
        command: Box<dyn Any + Send>,
    ) -> Result<Box<dyn Any + Send>, CqrsError> {
        let command = command
            .downcast::<C>()
            .map_err(|_| CqrsError::new("command downcast failed"))?;
        let res = self.inner.execute(*command).await?;
        Ok(Box::new(res))
    }
}

#[async_trait]
trait ErasedQueryHandler: Send + Sync + 'static {
    fn query_type_id(&self) -> TypeId;
    async fn execute_boxed(&self, query: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, CqrsError>;
}

struct QueryHandlerBox<H, Q>
where
    H: QueryHandler<Q>,
    Q: Query,
{
    inner: Arc<H>,
    _marker: std::marker::PhantomData<fn() -> Q>,
}

#[async_trait]
impl<H, Q> ErasedQueryHandler for QueryHandlerBox<H, Q>
where
    H: QueryHandler<Q>,
    Q: Query,
{
    fn query_type_id(&self) -> TypeId {
        TypeId::of::<Q>()
    }

    async fn execute_boxed(&self, query: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, CqrsError> {
        let query = query
            .downcast::<Q>()
            .map_err(|_| CqrsError::new("query downcast failed"))?;
        let res = self.inner.execute(*query).await?;
        Ok(Box::new(res))
    }
}

#[derive(Default)]
pub struct CommandBus {
    handlers: tokio::sync::RwLock<HashMap<TypeId, Arc<dyn ErasedCommandHandler>>>,
}

impl CommandBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register<C, H>(&self, handler: Arc<H>)
    where
        C: Command,
        H: CommandHandler<C>,
    {
        let boxed: Arc<dyn ErasedCommandHandler> = Arc::new(CommandHandlerBox::<H, C> {
            inner: handler,
            _marker: std::marker::PhantomData,
        });
        let mut guard = self.handlers.write().await;
        guard.insert(boxed.command_type_id(), boxed);
    }

    pub async fn execute<C>(&self, command: C) -> Result<C::Response, CqrsError>
    where
        C: Command,
    {
        let handler = {
            let guard = self.handlers.read().await;
            guard
                .get(&TypeId::of::<C>())
                .cloned()
                .ok_or_else(|| CqrsError::new("no command handler registered"))?
        };

        let boxed = handler.execute_boxed(Box::new(command)).await?;
        boxed
            .downcast::<C::Response>()
            .map(|b| *b)
            .map_err(|_| CqrsError::new("command response downcast failed"))
    }
}

#[async_trait]
impl Injectable for CommandBus {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[derive(Default)]
pub struct QueryBus {
    handlers: tokio::sync::RwLock<HashMap<TypeId, Arc<dyn ErasedQueryHandler>>>,
}

impl QueryBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register<Q, H>(&self, handler: Arc<H>)
    where
        Q: Query,
        H: QueryHandler<Q>,
    {
        let boxed: Arc<dyn ErasedQueryHandler> = Arc::new(QueryHandlerBox::<H, Q> {
            inner: handler,
            _marker: std::marker::PhantomData,
        });
        let mut guard = self.handlers.write().await;
        guard.insert(boxed.query_type_id(), boxed);
    }

    pub async fn execute<Q>(&self, query: Q) -> Result<Q::Response, CqrsError>
    where
        Q: Query,
    {
        let handler = {
            let guard = self.handlers.read().await;
            guard
                .get(&TypeId::of::<Q>())
                .cloned()
                .ok_or_else(|| CqrsError::new("no query handler registered"))?
        };

        let boxed = handler.execute_boxed(Box::new(query)).await?;
        boxed
            .downcast::<Q::Response>()
            .map(|b| *b)
            .map_err(|_| CqrsError::new("query response downcast failed"))
    }
}

#[async_trait]
impl Injectable for QueryBus {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::new())
    }
}

/// Nest-like CQRS module exporting `CommandBus` + `QueryBus`.
///
/// This crate intentionally keeps auto-discovery out of the initial version; handlers can be
/// registered imperatively (typically in a lifecycle hook or app bootstrap).
pub struct CqrsModule;

impl Module for CqrsModule {
    fn build() -> (ProviderRegistry, axum::Router) {
        let mut registry = ProviderRegistry::new();
        registry.register::<CommandBus>();
        registry.register::<QueryBus>();
        (registry, axum::Router::new())
    }

    fn exports() -> Vec<TypeId> {
        vec![TypeId::of::<CommandBus>(), TypeId::of::<QueryBus>()]
    }
}

impl ModuleGraph for CqrsModule {
    fn register_providers(registry: &mut ProviderRegistry) {
        registry.register::<CommandBus>();
        registry.register::<QueryBus>();
    }

    fn register_controllers(router: axum::Router, _registry: &ProviderRegistry) -> axum::Router {
        router
    }
}

