//! NestJS [`@nestjs/idempotency`](https://docs.nestjs.com/reliability/idempotency) analogue —
//! `Idempotency-Key` request deduplication.
//!
//! # Behavior
//!
//! 1. Missing key → pass through (or **400** if [`IdempotencyOptions::required`]).
//! 2. Completed key for the **same caller and same request** → replay stored status/body
//!    with header `idempotency-replayed: true`.
//! 3. Same key, different method, path, query, or body → **409** (`IDEMPOTENCY_KEY_MISMATCH`)
//!    without the stored body.
//! 4. In-flight key → **409** (`IDEMPOTENCY_IN_FLIGHT`).
//! 5. First writer → run handler, then [`IdempotencyStore::complete`].
//!
//! # Security
//!
//! The store key is bound to a hash of `Authorization` and `Cookie`, so a client-supplied
//! key is not a global lookup. [`IdempotencyOptions::scope`] is an extra partition (tenant,
//! route group). Prefer Redis TTL in multi-instance deploys (`cache-redis`). Redis
//! claim-or-read is one `EVAL`, so a peer cannot run the handler again between a `GET` of
//! the stored response and `SET NX` of the lock.
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
