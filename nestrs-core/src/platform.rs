/// Pluggable HTTP engine (NestJS “platform agnostic” idea). Only [`AxumHttpEngine`] is implemented today.
pub trait HttpServerEngine: Send + Sync + 'static {
    type Router: Send + 'static;
}

/// Default engine: Axum [`Router`](axum::Router).
pub struct AxumHttpEngine;

impl HttpServerEngine for AxumHttpEngine {
    type Router = axum::Router;
}
