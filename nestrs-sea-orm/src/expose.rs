//! Wire-schema helpers — NestRS `#[expose]` analogue for OpenAPI / JSON Schema.
//!
//! NestRS declares a field once on the SeaORM column and derives DTO + OpenAPI
//! + GraphQL. In nestrs the Rust type system keeps those as **one shared type**:
//! derive `Serialize` + `schemars::JsonSchema` on your entity `Model` (or a
//! dedicated view DTO), then feed it to OpenAPI via [`expose_schema`] /
//! [`nestrs_openapi::schema_entry`](https://docs.rs/nestrs-openapi) and reuse
//! the same type in GraphQL `SimpleObject` / resolvers.
//!
//! ```ignore
//! use nestrs_openapi::OpenApiOptions;
//! use nestrs_sea_orm::expose_schema;
//!
//! #[derive(Clone, Debug, Serialize, schemars::JsonSchema, DeriveEntityModel)]
//! #[sea_orm(table_name = "posts")]
//! pub struct Model { /* ... */ }
//!
//! let _opts = OpenApiOptions::default()
//!     .with_schemas([expose_schema::<Model>("Post")]);
//! ```

/// Build one OpenAPI `components.schemas` entry from a `JsonSchema` type.
///
/// Alias of the OpenAPI crate helper so SeaORM apps can depend on one
/// adapter surface for "expose this model".
#[cfg(feature = "expose")]
pub fn expose_schema<T: schemars::JsonSchema>(name: &str) -> (String, serde_json::Value) {
    (
        name.to_string(),
        serde_json::to_value(schemars::schema_for!(T)).expect("JsonSchema always serializes"),
    )
}

/// Documentation-only marker: prefer deriving `JsonSchema` on the model / DTO
/// and calling [`expose_schema`] (feature `expose`) or `nestrs_openapi::schema_entry`.
#[cfg(not(feature = "expose"))]
pub fn expose_schema_hint() -> &'static str {
    "enable nestrs-sea-orm feature `expose` (schemars) or use nestrs_openapi::schema_entry"
}
