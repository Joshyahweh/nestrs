//! DTO field metadata (validators, types).
//!
//! Mirrors the field-attr translation table in
//! `nestrs-macros/src/lib.rs:3481` (`convert_dto_field_attrs`). The list is
//! intentionally kept small and stable: every validator here is one the macro
//! actually recognizes. New validators should be added to both the macro and
//! this enum in the same change.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A single DTO field, with its type, optional inner type, and validators
/// derived from the field attributes the user wrote.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DtoField {
    /// Field name as written in the source (preserves Rust naming).
    pub name: String,
    /// Type as a string (e.g. `"String"`, `"i64"`, `"Option<String>"`).
    /// Inner generic args are kept; `Option<T>` is shown as a single type
    /// string rather than split apart.
    pub ty: String,
    /// Whether the field's type is `Option<_>`. The macro accepts both
    /// `Option<T>` and the field-level `IsOptional` marker — they mean the
    /// same thing to the validator.
    pub optional: bool,
    /// Validators applied to this field, in source order.
    pub validators: Vec<Validator>,
}

/// A single validator, normalized into a `(name, args)` shape so the model
/// can render or compare them uniformly.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Validator {
    IsString,
    IsEmail,
    IsNotEmpty,
    IsUuid,
    IsPositive,
    IsNegative,
    IsInt,
    IsNumber,
    IsBoolean,
    IsUrl,
    IsOptional,
    MinLength { value: u64 },
    MaxLength { value: u64 },
    Length { min: u64, max: u64 },
    Min { value: String },
    Max { value: String },
    Matches { pattern: String },
    Contains { substring: String },
    ValidateNested,
    Expose,
    Exclude,
    /// An attribute the parser doesn't recognize. Surfaced so the model can
    /// mention it ("this field has `MyCustomValidator` — I don't know what
    /// that does") rather than silently dropping it.
    Unknown { name: String, args: String },
}

/// Summary of a DTO struct, suitable for listing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DtoSummary {
    /// Struct name as written.
    pub name: String,
    /// Module path (e.g. `"myapp::dto"`), inferred from the file path.
    pub module_path: String,
    /// Source file (relative to the workspace root).
    pub file: String,
    /// Field count, including ones without validators.
    pub field_count: usize,
    /// Whether `#[dto(allow_unknown_fields)]` was set.
    pub allow_unknown_fields: bool,
    /// Whether `#[dto(expose_only)]` was set.
    pub expose_only: bool,
}
