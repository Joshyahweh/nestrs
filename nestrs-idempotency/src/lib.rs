//! NestJS [`@nestjs/idempotency`](https://docs.nestjs.com/reliability/idempotency) analogue —
//! `Idempotency-Key` request deduplication.
//!
//! # Behavior
//!
//! 1. Missing key → pass through (or **400** if [`IdempotencyOptions::required`]).
//! 2. Completed key → replay stored status/body with header `idempotency-replayed: true`.
//! 3. In-flight key → **409** (`IDEMPOTENCY_IN_FLIGHT`).
//! 4. First writer → run handler, then [`IdempotencyStore::complete`].
//!
//! # Security
//!
//! Always set [`IdempotencyOptions::scope`] to the authenticated principal so tenants cannot
//! replay another user's response. Prefer Redis TTL in multi-instance deploys (`cache-redis`).
//!
//! # Middleware
//!
//! ```ignore
//! use axum::middleware::from_fn_with_state;
//! use nestrs_idempotency::{idempotency_middleware, IdempotencyOptions};
//! use std::sync::Arc;
//!
//! let opts = Arc::new(IdempotencyOptions::default());
//! let router = router.layer(from_fn_with_state(opts, idempotency_middleware));
//! ```
//!
//! Umbrella feature: `nestrs` / `idempotency` → `nestrs::idempotency`.

#![doc(html_root_url = "https://docs.rs/nestrs-idempotency/1.7.1")]

mod middleware;
mod module;
mod options;
mod store;

pub use middleware::idempotency_middleware;
pub use module::IdempotencyModule;
pub use options::IdempotencyOptions;
pub use store::{IdempotencyRecord, IdempotencyStore, InMemoryIdempotencyStore};

#[cfg(feature = "cache-redis")]
pub use store::RedisIdempotencyStore;

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
