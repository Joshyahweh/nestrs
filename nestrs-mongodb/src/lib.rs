//! Mongoose-style MongoDB adapter for the [`nestrs`](https://crates.io/crates/nestrs)
//! framework — the Rust equivalent of NestJS's
//! [`@nestjs/mongoose`](https://docs.nestjs.com/techniques/mongodb).
//!
//! See the [`README`](https://github.com/Joshyahweh/nestrs/tree/main/nestrs-mongodb)
//! for the high-level API. This crate is intentionally small at the entry point;
//! the public surface is split across modules that come online in subsequent phases:
//!
//! - [`client`] — connection options, client lifecycle, `MongoModule::for_root`.
//! - [`schema`] — `Document` trait, `#[schema(...)]` / `#[prop(...)]` derive surface.
//! - [`repository`] — typed `MongoRepository<T>` CRUD wrapper.
//! - [`module`] — `MongoModule`, `forRootAsync`, `forFeature` model registration,
//!   `#[inject_model]` macro surface.
//!
//! Feature flags:
//! - `default = []` — TLS via `rustls`, BSON `compat-3-0-0` codec.
//! - `dns-resolver` — `mongodb+srv://` Atlas-style seed lists (pulls `hickory-*`).
//! - `all` — convenience feature for everything.

#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod client;
pub mod error;
pub mod repository;
pub mod schema;

pub use client::{MongoModule, MongoOptions, MongoService};
pub use error::MongoError;
pub use repository::{Filter, MongoRepository, Update};
pub use schema::{Document, Schema};

// Re-export the upstream `mongodb` and `bson` crates so callers don't have to
// add them as direct deps. The `MongoRepository<T>` typed wrapper uses these
// directly for things like `Database`, `Collection`, `bson::doc!`.
pub use bson;
pub use mongodb;