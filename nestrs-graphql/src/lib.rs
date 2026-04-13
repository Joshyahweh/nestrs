//! GraphQL integration for `nestrs` using `async-graphql`.
//!
//! ## Ecosystem (Nest chapter vs Rust)
//!
//! - **Federation**: use [`async_graphql`](https://docs.rs/async-graphql) federation / subgraph features, or place
//!   [`export_schema_sdl`](crate::export_schema_sdl) output behind **Apollo Router** / **GraphOS** — there is no separate “nestrs federation” crate.
//! - **Plugins**: implement [`Extension`](async_graphql::extensions::Extension) / [`ExtensionFactory`](async_graphql::extensions::ExtensionFactory)
//!   and register with [`SchemaBuilder::extension`](async_graphql::SchemaBuilder::extension). [`Analyzer`] is wired by [`with_production_graphql_limits`].
//! - **SDL / codegen**: export with [`export_schema_sdl`] or [`export_schema_sdl_with_options`]; run **graphql-client**, **cynic**, or **async-graphql**’s own derives in your repo.
//! - **Mapped / custom scalars**: use `#[Scalar]`, newtypes, and `Object`/`InputObject` as in the async-graphql book.
//! - **Field middleware / guards**: use the `guard` argument on `#[graphql(guard = "...")]` / `FieldGuard` patterns from async-graphql (authorization lives in the schema layer).
//!
//! The HTTP adapter here stays small: Axum router + optional Playground + batch execution.

pub mod builder_help;
pub mod limits;
pub mod router_options;
pub mod sdl;

pub use builder_help::with_production_graphql_limits;
pub use limits::{with_default_limits, Analyzer, DEFAULT_MAX_COMPLEXITY, DEFAULT_MAX_DEPTH};
pub use router_options::{graphql_router_with_options, GraphQlHttpOptions};
pub use sdl::{export_schema_sdl, export_schema_sdl_with_options, SDLExportOptions};

pub use async_graphql::{BatchRequest, ObjectType, Schema, SubscriptionType};
use axum::Router;

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
