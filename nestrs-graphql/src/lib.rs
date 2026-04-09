//! Optional GraphQL integration surface for nestrs (Phase 4 roadmap crate).
//!
//! This crate is intentionally small for now; async-graphql integration can be layered in without
//! breaking the public nestrs crate.

use async_trait::async_trait;

#[async_trait]
pub trait Resolver: Send + Sync + 'static {
    async fn field_names(&self) -> Vec<&'static str>;
}
