//! MongoDB integration (NestJS **MongooseModule** analogue). Features: **`mongo`** (TLS + BSON compat),
//! optional **`mongo-dns`** for **`mongodb+srv://`** / SRV seed lists (adds the driver's **`dns-resolver`** stack).
//!
//! Thin re-export shim over the `nestrs-mongodb` workspace member. Real
//! implementation lives in `nestrs_mongodb::*`; the umbrella keeps the same
//! paths so existing code
//! (`nestrs::MongoModule::for_root`, `MongoService`, `MongoRepository`)
//! keeps compiling unchanged. Feature gates on the umbrella's `mongo` /
//! `mongo-dns` flags mirror the underlying crate's feature flags.

#[cfg(feature = "mongo")]
pub use nestrs_mongodb::{
    client::{MongoModule, MongoOptions, MongoService},
    error::{MongoError, Result as MongoResult},
    repository::{Filter, MongoRepository, Update},
    schema::{Document, Schema},
};