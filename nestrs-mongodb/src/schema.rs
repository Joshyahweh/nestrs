//! Schema traits and derive surface.
//!
//! The `Document` trait marks a struct as a Mongoose-style Mongo document and
//! gives the typed `MongoRepository<T>` everything it needs to drive the
//! upstream driver's `Collection<T>`. The full derive surface (`#[schema]`,
//! `#[prop]`) lands in Wave 7.6 Phase C; this module ships the trait contract
//! so downstream types can already be written and the rest of the crate
//! compiles against a stable surface.

use bson::{Bson, Document as BsonDoc};

/// Implemented by types that can be persisted as a MongoDB document.
///
/// The trait is the typed counterpart of Mongoose's `@Schema` decorator. It
/// carries the collection name (via [`Schema::collection_name`]) and provides
/// conversion to / from a raw `bson::Document` for the times you want to
/// drop down to the driver directly.
///
/// Users do not normally implement `Document` by hand. The derive macro (added
/// in Phase C) wires up `collection_name` from the `#[schema(collection = …)]`
/// attribute, the auto-generated `_id` field, and the `to_bson` / `from_bson`
/// round-trip through `serde`.
pub trait Document: Sized + Send + Sync + 'static {
    /// Mongo collection name. Maps to Mongoose's `MongooseModule.forFeature`
    /// registration key.
    fn collection_name() -> &'static str;

    /// Convert into a raw `bson::Document`. The default impl delegates to
    /// `bson::to_document`; override only if you need full control over the
    /// BSON shape (rare — usually `#[serde(rename = "...")]` is enough).
    fn to_bson(&self) -> Result<BsonDoc, bson::ser::Error> {
        bson::to_document(self)
    }

    /// Build from a raw `bson::Document`. The default impl delegates to
    /// `bson::from_document`.
    fn from_bson(doc: &BsonDoc) -> Result<Self, bson::de::Error> {
        bson::from_document(doc)
    }

    /// Convert into a generic `Bson` value (used when the field type is
    /// `Bson` itself). Default impl serializes via `serde`.
    fn to_bson_value(&self) -> Result<Bson, bson::ser::Error> {
        bson::to_bson(self)
    }
}

/// Re-export of `bson::Document` under the `nestrs_mongodb::schema::Document`
/// path so callers writing raw filters / updates don't need a separate
/// `bson::Document` import alongside `MongoRepository`.
pub type BsonDocument = BsonDoc;

/// Convenience re-export of [`Document`]'s associated `collection_name` so
/// callers can write `nestrs_mongodb::schema::collection::<User>()` instead
/// of `<User as Document>::collection_name()`.
pub fn collection<T: Document>() -> &'static str {
    T::collection_name()
}

/// Trait alias for the shape required by `MongoRepository<T>`: `T: Document`
/// is what we need, but exposing it as a trait alias keeps the bounds on
/// `MongoRepository::new` readable.
pub trait Schema: Document {}
impl<T: Document> Schema for T {}