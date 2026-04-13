//! Opinionated [`SchemaBuilder`] chaining for production services.

use async_graphql::extensions::Analyzer;
use async_graphql::SchemaBuilder;

use crate::limits::{DEFAULT_MAX_COMPLEXITY, DEFAULT_MAX_DEPTH};

/// Depth + complexity limits plus the built-in [`Analyzer`] extension (complexity/depth in response extensions).
pub fn with_production_graphql_limits<Q, M, S>(
    builder: SchemaBuilder<Q, M, S>,
) -> SchemaBuilder<Q, M, S> {
    builder
        .limit_depth(DEFAULT_MAX_DEPTH)
        .limit_complexity(DEFAULT_MAX_COMPLEXITY)
        .extension(Analyzer)
}
