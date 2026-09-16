//! Re-export of drizzle-orm's `table!` macro and column types under
//! `nestrs_drizzle::schema::*` so callers don't need to import the
//! drizzle-orm crate directly to define a schema.
//!
//! The `table!` macro itself lives in `drizzle-orm`; this module just
//! re-exports it plus a few common column type aliases. The actual
//! SQL-flavor-specific helpers (`Int4`, `Text`, `Varchar`, …) are
//! re-exported from each backend module via `nestrs_drizzle::*`.

pub use drizzle_orm::table;
pub use drizzle_orm::Column;

/// Common SQL identifier type, re-exported under
/// `nestrs_drizzle::schema::SqlId` for parity with the rest of the
/// column-type aliases.
pub use drizzle_orm::sql_types::Integer as Int4;
pub use drizzle_orm::sql_types::Text;
pub use drizzle_orm::sql_types::BigInt as Int8;
pub use drizzle_orm::sql_types::SmallInt as Int2;
pub use drizzle_orm::sql_types::Boolean as Bool;
pub use drizzle_orm::sql_types::Timestamp;
pub use drizzle_orm::sql_types::Varchar;