//! Production-oriented GraphQL limits for [async-graphql](https://docs.rs/async-graphql).
//!
//! Depth and complexity are enforced on [`SchemaBuilder`](async_graphql::SchemaBuilder) (see
//! [`limit_depth`](async_graphql::SchemaBuilder::limit_depth) and
//! [`limit_complexity`](async_graphql::SchemaBuilder::limit_complexity)), not via extensions.

pub use async_graphql::extensions::Analyzer;
use async_graphql::SchemaBuilder;

/// Suggested defaults for public HTTP APIs (tune per service).
pub const DEFAULT_MAX_DEPTH: usize = 64;
pub const DEFAULT_MAX_COMPLEXITY: usize = 512;

/// Apply default depth and complexity limits to a schema under construction.
pub fn with_default_limits<Q, M, S>(builder: SchemaBuilder<Q, M, S>) -> SchemaBuilder<Q, M, S> {
    builder
        .limit_depth(DEFAULT_MAX_DEPTH)
        .limit_complexity(DEFAULT_MAX_COMPLEXITY)
}
