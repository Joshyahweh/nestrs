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
//! The store key is bound to a hash of every `Authorization` and `Cookie` value, so a
//! client-supplied key is not a global lookup and a second header cannot leave the real
//! session cookie outside the partition. [`IdempotencyOptions::scope`] is an extra
//! partition (tenant, route group). Bodies larger than
//! [`IdempotencyOptions::max_body_bytes`] are **413** and are not executed. Prefer Redis
//! TTL in multi-instance deploys (`cache-redis`). Claim-or-read is one `EVAL`, and
//! `complete` publishes the body and deletes the lock only while it still holds the
//! fence token from that claim.
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
pub use options::{IdempotencyOptions, DEFAULT_MAX_BODY_BYTES, MAX_IDEMPOTENCY_KEY_BYTES};
pub use store::{IdempotencyClaim, IdempotencyRecord, IdempotencyStore, InMemoryIdempotencyStore};

#[cfg(feature = "cache-redis")]
pub use store::RedisIdempotencyStore;

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
