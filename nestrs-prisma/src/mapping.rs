//! Prisma-style database mapping helpers:
//! - `@map` / `@@map` equivalents for model and field names
//! - enum and enum-value mapping (`@@map` / `@map`)
//! - deterministic default index/constraint naming conventions
//! - `name:` selector naming for compound `@@id` / `@@unique`
//!
//! This module does not parse `schema.prisma`; it provides utilities for
//! Rust-side code generation and SQL helper layers.

use std::collections::BTreeMap;

/// In-memory representation of Prisma-style `@@map` / `@map` for one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMapping {
    model_name: String,
    db_table_name: String,
    field_to_column: BTreeMap<String, String>,
    column_to_field: BTreeMap<String, String>,
}

impl ModelMapping {
    /// Start with identity mapping (`model` -> `model`, each field resolved lazily to itself).
    pub fn new(model_name: impl Into<String>) -> Self {
        let model_name = model_name.into();
        Self {
            db_table_name: model_name.clone(),
            model_name,
            field_to_column: BTreeMap::new(),
            column_to_field: BTreeMap::new(),
        }
    }

    /// Prisma `@@map("...")` equivalent for table/collection name.
    pub fn map_model(mut self, table_or_collection: impl Into<String>) -> Self {
        self.db_table_name = table_or_collection.into();
        self
    }

    /// Prisma `@map("...")` equivalent for one field/column mapping.
    pub fn map_field(
        mut self,
        field_name: impl Into<String>,
        column_name: impl Into<String>,
    ) -> Self {
        let field_name = field_name.into();
        let column_name = column_name.into();
        self.field_to_column
            .insert(field_name.clone(), column_name.clone());
        self.column_to_field.insert(column_name, field_name);
        self
    }

    /// Logical model name from schema/API.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Physical table/collection name in the database.
    pub fn db_table_name(&self) -> &str {
        &self.db_table_name
    }

    /// Resolve schema field -> DB column (`@map`), defaulting to the same name.
    pub fn db_column_name<'a>(&'a self, field_name: &'a str) -> &'a str {
        self.field_to_column
            .get(field_name)
            .map(String::as_str)
            .unwrap_or(field_name)
    }

    /// Resolve DB column -> schema field (`@map` reverse), defaulting to the same name.
    pub fn schema_field_name<'a>(&'a self, column_name: &'a str) -> &'a str {
        self.column_to_field
            .get(column_name)
            .map(String::as_str)
            .unwrap_or(column_name)
    }
}

/// Prisma-style enum mapping (`@@map` for enum + `@map` for values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumMapping {
    schema_enum_name: String,
    db_enum_name: String,
    schema_to_db_value: BTreeMap<String, String>,
    db_to_schema_value: BTreeMap<String, String>,
}

impl EnumMapping {
    pub fn new(schema_enum_name: impl Into<String>) -> Self {
        let schema_enum_name = schema_enum_name.into();
        Self {
            db_enum_name: schema_enum_name.clone(),
            schema_enum_name,
            schema_to_db_value: BTreeMap::new(),
            db_to_schema_value: BTreeMap::new(),
        }
    }

    /// Prisma enum-level `@@map("...")`.
    pub fn map_enum(mut self, db_enum_name: impl Into<String>) -> Self {
        self.db_enum_name = db_enum_name.into();
        self
    }

    /// Prisma enum-value `@map("...")`.
    pub fn map_value(
        mut self,
        schema_value: impl Into<String>,
        db_value: impl Into<String>,
    ) -> Self {
        let schema_value = schema_value.into();
        let db_value = db_value.into();
        self.schema_to_db_value
            .insert(schema_value.clone(), db_value.clone());
        self.db_to_schema_value.insert(db_value, schema_value);
        self
    }

    pub fn schema_enum_name(&self) -> &str {
        &self.schema_enum_name
    }

    pub fn db_enum_name(&self) -> &str {
        &self.db_enum_name
    }

    /// Resolve schema enum variant -> DB value, defaulting to schema variant.
    pub fn db_value<'a>(&'a self, schema_value: &'a str) -> &'a str {
        self.schema_to_db_value
            .get(schema_value)
            .map(String::as_str)
            .unwrap_or(schema_value)
    }

    /// Resolve DB value -> schema enum variant, defaulting to DB value.
    pub fn schema_value<'a>(&'a self, db_value: &'a str) -> &'a str {
        self.db_to_schema_value
            .get(db_value)
            .map(String::as_str)
            .unwrap_or(db_value)
    }
}

/// Prisma index/constraint naming convention categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConstraintKind {
    PrimaryKey,
    UniqueConstraint,
    NonUniqueIndex,
    ForeignKey,
}

impl ConstraintKind {
    fn suffix(self) -> &'static str {
        match self {
            ConstraintKind::PrimaryKey => "_pkey",
            ConstraintKind::UniqueConstraint => "_key",
            ConstraintKind::NonUniqueIndex => "_idx",
            ConstraintKind::ForeignKey => "_fkey",
        }
    }
}

/// Build Prisma-style default name from physical DB identifiers.
///
/// Naming basis:
/// - `PrimaryKey`: `{table}_pkey`
/// - `UniqueConstraint`: `{table}_{column_names}_key`
/// - `NonUniqueIndex`: `{table}_{column_names}_idx`
/// - `ForeignKey`: `{table}_{column_names}_fkey`
///
/// If the resulting name exceeds `max_identifier_len`, this helper trims the
/// part before the suffix (same strategy Prisma docs describe).
pub fn prisma_default_constraint_name(
    table_name_in_db: &str,
    column_names_in_db: &[&str],
    kind: ConstraintKind,
    max_identifier_len: usize,
) -> String {
    let suffix = kind.suffix();
    let mut prefix = match kind {
        ConstraintKind::PrimaryKey => table_name_in_db.to_string(),
        _ => {
            let cols = column_names_in_db.join("_");
            if cols.is_empty() {
                table_name_in_db.to_string()
            } else {
                format!("{table_name_in_db}_{cols}")
            }
        }
    };

    let full_len = prefix.len() + suffix.len();
    if full_len > max_identifier_len {
        let keep = max_identifier_len.saturating_sub(suffix.len());
        prefix.truncate(keep);
    }

    format!("{prefix}{suffix}")
}

/// Returns whether Prisma would need to render `map:` for a constraint/index
/// when introspecting (name differs from default deterministic convention).
pub fn should_render_constraint_map_argument(
    actual_db_name: &str,
    table_name_in_db: &str,
    column_names_in_db: &[&str],
    kind: ConstraintKind,
    max_identifier_len: usize,
) -> bool {
    actual_db_name
        != prisma_default_constraint_name(
            table_name_in_db,
            column_names_in_db,
            kind,
            max_identifier_len,
        )
}

/// Resolves physical DB name with Prisma semantics:
/// explicit `map` wins; otherwise use deterministic default name.
pub fn resolve_constraint_db_name(
    map_argument: Option<&str>,
    table_name_in_db: &str,
    column_names_in_db: &[&str],
    kind: ConstraintKind,
    max_identifier_len: usize,
) -> String {
    map_argument.map(str::to_owned).unwrap_or_else(|| {
        prisma_default_constraint_name(
            table_name_in_db,
            column_names_in_db,
            kind,
            max_identifier_len,
        )
    })
}

/// Default selector key for compound `@@id` / `@@unique` in Prisma Client API.
///
/// Example: `["firstName", "lastName"]` -> `"firstName_lastName"`.
pub fn default_compound_selector_name(schema_field_names: &[&str]) -> String {
    schema_field_names.join("_")
}

/// Resolve compound selector key using Prisma `name:` semantics.
///
/// `name` customizes client API key, while `map` customizes database object name.
pub fn resolve_compound_selector_name(
    name_argument: Option<&str>,
    schema_field_names: &[&str],
) -> String {
    name_argument
        .map(str::to_owned)
        .unwrap_or_else(|| default_compound_selector_name(schema_field_names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_mapping_works() {
        let m = ModelMapping::new("Comment")
            .map_model("comments")
            .map_field("content", "comment_text")
            .map_field("email", "commenter_email");

        assert_eq!(m.model_name(), "Comment");
        assert_eq!(m.db_table_name(), "comments");
        assert_eq!(m.db_column_name("content"), "comment_text");
        assert_eq!(m.db_column_name("type"), "type");
        assert_eq!(m.schema_field_name("commenter_email"), "email");
        assert_eq!(m.schema_field_name("unknown_col"), "unknown_col");
    }

    #[test]
    fn enum_mapping_works() {
        let e = EnumMapping::new("Type")
            .map_enum("comment_source_enum")
            .map_value("Twitter", "comment_twitter");

        assert_eq!(e.schema_enum_name(), "Type");
        assert_eq!(e.db_enum_name(), "comment_source_enum");
        assert_eq!(e.db_value("Twitter"), "comment_twitter");
        assert_eq!(e.db_value("Blog"), "Blog");
        assert_eq!(e.schema_value("comment_twitter"), "Twitter");
        assert_eq!(e.schema_value("other"), "other");
    }

    #[test]
    fn default_constraint_names_follow_prisma_shape() {
        assert_eq!(
            prisma_default_constraint_name("User", &["id"], ConstraintKind::PrimaryKey, 63),
            "User_pkey"
        );
        assert_eq!(
            prisma_default_constraint_name(
                "User",
                &["firstName", "lastName"],
                ConstraintKind::UniqueConstraint,
                63
            ),
            "User_firstName_lastName_key"
        );
        assert_eq!(
            prisma_default_constraint_name("User", &["age"], ConstraintKind::NonUniqueIndex, 63),
            "User_age_idx"
        );
        assert_eq!(
            prisma_default_constraint_name("Post", &["authorName"], ConstraintKind::ForeignKey, 63),
            "Post_authorName_fkey"
        );
    }

    #[test]
    fn default_constraint_name_is_trimmed_before_suffix() {
        let table = "VeryLongTableName";
        let cols = ["veryLongColumnName", "anotherColumnName"];
        let out = prisma_default_constraint_name(table, &cols, ConstraintKind::NonUniqueIndex, 20);
        assert!(out.ends_with("_idx"));
        assert!(out.len() <= 20);
    }

    #[test]
    fn map_argument_rendering_detection_matches_default() {
        let should_hide = should_render_constraint_map_argument(
            "Post_title_authorName_idx",
            "Post",
            &["title", "authorName"],
            ConstraintKind::NonUniqueIndex,
            63,
        );
        let should_show = should_render_constraint_map_argument(
            "My_Custom_Index_Name",
            "Post",
            &["title", "authorName"],
            ConstraintKind::NonUniqueIndex,
            63,
        );
        assert!(!should_hide);
        assert!(should_show);
    }

    #[test]
    fn resolve_constraint_name_prefers_map() {
        let from_default = resolve_constraint_db_name(
            None,
            "User",
            &["name"],
            ConstraintKind::UniqueConstraint,
            63,
        );
        let from_map = resolve_constraint_db_name(
            Some("unique_user_name"),
            "User",
            &["name"],
            ConstraintKind::UniqueConstraint,
            63,
        );
        assert_eq!(from_default, "User_name_key");
        assert_eq!(from_map, "unique_user_name");
    }

    #[test]
    fn compound_selector_name_supports_name_argument() {
        assert_eq!(
            default_compound_selector_name(&["firstName", "lastName"]),
            "firstName_lastName"
        );
        assert_eq!(
            resolve_compound_selector_name(None, &["firstName", "lastName"]),
            "firstName_lastName"
        );
        assert_eq!(
            resolve_compound_selector_name(Some("fullName"), &["firstName", "lastName"]),
            "fullName"
        );
    }
}
