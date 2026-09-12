//! GraphQL integration for `nestrs` using `async-graphql`.
//!
//! ## Ecosystem (Nest chapter vs Rust)
//!
//! - **Federation**: use the in-crate `federation` module
//!   ([`federation_router`] / [`SubgraphSpec`]) when the
//!   `federation-gateway` feature is enabled, or place
//!   [`export_schema_sdl`] output behind **Apollo Router** / **GraphOS**
//!   for the externally-routed case.
//! - **Plugins**: implement [`async_graphql::extensions::Extension`] /
//!   [`async_graphql::extensions::ExtensionFactory`] and register with
//!   [`async_graphql::SchemaBuilder::extension`].
//!   [`Analyzer`] is wired by [`with_production_graphql_limits`].
//! - **SDL / codegen**: export with [`export_schema_sdl`] or [`export_schema_sdl_with_options`]; run **graphql-client**, **cynic**, or **async-graphql**’s own derives in your repo.
//! - **Mapped / custom scalars**: use `#[Scalar]`, newtypes, and `Object`/`InputObject` as in the async-graphql book.
//! - **Field middleware / guards**: use the `guard` argument on `#[graphql(guard = "...")]` / `FieldGuard` patterns from async-graphql (authorization lives in the schema layer).
//!
//! The HTTP adapter here stays small: Axum router + optional Playground + batch execution.

pub mod builder_help;
#[cfg(feature = "federation-gateway")]
pub mod federation;
pub mod limits;
pub mod router_options;
pub mod sdl;

pub use builder_help::with_production_graphql_limits;
#[cfg(feature = "federation-gateway")]
pub use federation::{
    federation_router, federation_router_with_hook, federation_router_with_options, EntityResolver,
    FederationConfig, FederationError, SubgraphSpec,
};
pub use limits::{with_default_limits, Analyzer, DEFAULT_MAX_COMPLEXITY, DEFAULT_MAX_DEPTH};
pub use router_options::{graphql_router_with_options, GraphQlHttpOptions};
pub use sdl::{export_schema_sdl, export_schema_sdl_with_options, SDLExportOptions};

pub use async_graphql::{
    BatchRequest, BatchResponse, Error, ObjectType, Request, Response, Schema, SubscriptionType,
    Value,
};

#[cfg(feature = "dataloader")]
pub mod data_loader;
use axum::Router;
#[cfg(feature = "dataloader")]
pub use data_loader::{data_loader, data_loader_cached, DataLoader, DataLoaderRegistry, Loader};

pub fn graphql_router<Q, Mutation, Subscription>(
    schema: Schema<Q, Mutation, Subscription>,
    path: impl Into<String>,
) -> Router
where
    Q: ObjectType + Send + Sync + 'static,
    Mutation: ObjectType + Send + Sync + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    graphql_router_with_options(schema, path, GraphQlHttpOptions::default())
}

// `graphql-authz` feature: low-level trait-based hook that the main
// `nestrs` crate's typed wrappers (in `nestrs::graphql_authz`) build on.
// Exposed here so the main crate can wire the hook into the GraphQL
// router without depending on `nestrs` (Cargo would reject the cycle).
#[cfg(feature = "graphql-authz")]
pub mod gql_data_context;
#[cfg(feature = "graphql-authz")]
pub use gql_data_context::{graphql_router_with_hook, GqlHandlerHook, NoopHook};
