//! Drizzle ORM adapter for the [`nestrs`](https://crates.io/crates/nestrs)
//! framework — the Rust equivalent of NestJS's `@nestjs/typeorm` when
//! Drizzle is the chosen query builder.
//!
//! See the [`README`](https://github.com/Joshyahweh/nestrs/tree/main/nestrs-drizzle)
//! for the high-level API. This crate re-exports `drizzle_orm::*` at the
//! crate root and adds:
//!
//! - [`client`] — connection options, client lifecycle, `DrizzleModule::for_root`.
//! - [`schema`] — re-exports of `drizzle_orm::table!` + common column types.
//! - [`error`] — `DrizzleError` enum.
//!
//! Feature flags:
//! - `default = []` — base crate, no SQL backend enabled.
//! - `postgres` — `drizzle-orm/postgres` (sqlx postgres driver).
//! - `mysql` — `drizzle-orm/mysql` (sqlx mysql driver).
//! - `sqlite` — `drizzle-orm/sqlite` (sqlx sqlite driver).
//! - `all` — convenience feature for all three backends.

#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod client;
pub mod error;
pub mod schema;

pub use client::{DrizzleModule, DrizzleOptions, DrizzleService};
pub use error::{DrizzleError, Result as DrizzleResult};
pub use schema::Column;

// Re-export the upstream `drizzle_orm` crate so callers don't have to add
// it as a direct dep. Most of the typed-query / table-macro surface lives
// there; `nestrs-drizzle` adds the boot-time module / service shape.
pub use drizzle_orm;