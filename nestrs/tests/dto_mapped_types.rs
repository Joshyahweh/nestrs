//! Wave 7.5 / 8.1 — mapped-type macros.
//!
//! Mirrors NestJS's `@nestjs/mapped-types`:
//!   - `#[nestrs::partial_type]`     — every field becomes `Option<T>`
//!     (already-`Option` fields are not double-wrapped).
//!   - `#[nestrs::omit_type(a, b)]`  — drop named fields.
//!   - `#[nestrs::pick_type(a, b)]`  — keep only named fields.
//!   - `#[nestrs::intersection_type]` — flatten ≥2 parent DTOs into one JSON
//!     object (`IntersectionType(A, B)`). Parent DTOs must use
//!     `#[dto(allow_unknown_fields)]` so sibling flattened keys are not
//!     rejected as unknown.
//!
//! Partial/Omit/Pick emit the same derive set as `#[dto]` plus
//! `#[serde(deny_unknown_fields)]`. Intersection omits `deny_unknown_fields`
//! because serde forbids it on structs that contain `#[serde(flatten)]`.

use nestrs::prelude::*;
use nestrs::schemars;
use serde_json::Value;
use validator::Validate;

// ---------------------------------------------------------------------------
// Source shapes
// ---------------------------------------------------------------------------

#[dto]
struct CreateUserDto {
    #[IsEmail]
    email: String,
    #[MinLength(2)]
    name: String,
    age: i32,
    #[IsOptional]
    nickname: Option<String>,
}

// ---------------------------------------------------------------------------
// `#[partial_type]`
// ---------------------------------------------------------------------------

#[nestrs::partial_type]
struct UpdateUserDto {
    #[IsEmail]
    email: String,
    #[MinLength(2)]
    name: String,
    age: i32,
    #[IsOptional]
    nickname: Option<String>,
}

#[test]
fn partial_type_wraps_non_optional_fields() {
    // All four fields on UpdateUserDto must be assignable from the wrapped
    // types. If `#[partial_type]` did NOT wrap `email` / `name` / `age` in
    // `Option`, this struct-literal would not compile.
    let dto = UpdateUserDto {
        email: None,
        name: None,
        age: None,
        nickname: None,
    };
    assert!(dto.email.is_none());
    assert!(dto.name.is_none());
    assert!(dto.age.is_none());
    assert!(dto.nickname.is_none());
}

#[test]
fn partial_type_skips_already_optional_fields() {
    // `nickname` was `Option<String>` on the source shape. Wrapping it again
    // would yield `Option<Option<String>>`, which is exactly the bug
    // `PartialType` in TS avoids — verify the field is still flat
    // `Option<String>` by round-tripping through serde.
    let dto = UpdateUserDto {
        email: Some("ada@example.com".to_string()),
        name: Some("Ada".to_string()),
        age: Some(36),
        nickname: Some("ada".to_string()),
    };
    let json = serde_json::to_value(&dto).expect("serializes");
    let obj = json.as_object().expect("object");
    // `nickname: Some("ada".to_string())` only typechecks if the field is
    // `Option<String>`, not `Option<Option<String>>`. Serde then emits a
    // JSON string (not an object / nested null).
    assert_eq!(obj["nickname"], Value::String("ada".to_string()));
}

#[test]
fn partial_type_preserves_is_email_validator() {
    let dto = UpdateUserDto {
        email: Some("definitely-not-an-email".to_string()),
        name: Some("Ada".to_string()),
        age: Some(36),
        nickname: None,
    };
    let err = dto.validate().expect_err("bad email must fail validation");
    let fields = err.field_errors();
    assert!(
        fields.contains_key("email"),
        "IsEmail should still fire on the wrapped Option<String>; got {:?}",
        fields.keys().collect::<Vec<_>>()
    );
}

#[test]
fn partial_type_preserves_min_length_validator() {
    let dto = UpdateUserDto {
        email: Some("ada@example.com".to_string()),
        name: Some("A".to_string()), // 1 char, fails MinLength(2)
        age: Some(36),
        nickname: None,
    };
    let err = dto
        .validate()
        .expect_err("1-char name must fail MinLength(2)");
    assert!(
        err.field_errors().contains_key("name"),
        "MinLength should still fire on the wrapped Option<String>"
    );
}

#[test]
fn partial_type_skips_validators_when_none() {
    // An `Option` field with `None` skips its inner validator — this is
    // exactly the contract that makes PartialType useful for PATCH bodies.
    let dto = UpdateUserDto {
        email: None, // no validation when absent
        name: None,
        age: None,
        nickname: None,
    };
    dto.validate()
        .expect("all-None PATCH body must validate cleanly");
}

// ---------------------------------------------------------------------------
// `#[omit_type(...)]`
// ---------------------------------------------------------------------------

#[nestrs::omit_type(nickname)]
struct CreateUserNoNicknameDto {
    #[IsEmail]
    email: String,
    #[MinLength(2)]
    name: String,
    age: i32,
    #[IsOptional]
    nickname: Option<String>,
}

#[test]
fn omit_type_drops_named_fields() {
    // Construct without `nickname` — if the field was still present this
    // struct-literal would not compile.
    let dto = CreateUserNoNicknameDto {
        email: "ada@example.com".to_string(),
        name: "Ada".to_string(),
        age: 36,
    };
    assert_eq!(dto.email, "ada@example.com");
    assert_eq!(dto.age, 36);
}

#[test]
fn omit_type_keeps_validators_on_remaining_fields() {
    let dto = CreateUserNoNicknameDto {
        email: "not-an-email".to_string(),
        name: "Ada".to_string(),
        age: 36,
    };
    let err = dto.validate().expect_err("bad email must fail");
    assert!(err.field_errors().contains_key("email"));
}

// ---------------------------------------------------------------------------
// `#[pick_type(...)`
// ---------------------------------------------------------------------------

#[nestrs::pick_type(email, name)]
struct UserIdentityDto {
    #[IsEmail]
    email: String,
    #[MinLength(2)]
    name: String,
    age: i32,
    #[IsOptional]
    nickname: Option<String>,
}

#[test]
fn pick_type_keeps_only_named_fields() {
    // Construct with only `email` + `name` — if `age` or `nickname` were
    // still present this struct-literal would not compile.
    let dto = UserIdentityDto {
        email: "ada@example.com".to_string(),
        name: "Ada".to_string(),
    };
    assert_eq!(dto.email, "ada@example.com");
    assert_eq!(dto.name, "Ada");
}

#[test]
fn pick_type_serializes_only_picked_fields() {
    let dto = UserIdentityDto {
        email: "ada@example.com".to_string(),
        name: "Ada".to_string(),
    };
    let json = serde_json::to_value(&dto).expect("serializes");
    let obj = json.as_object().expect("object");
    let mut keys: Vec<_> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["email", "name"],
        "only picked fields in the serialized JSON"
    );
}

// ---------------------------------------------------------------------------
// schemars integration: PartialType makes every field optional in the
// reflected schema; PickType / OmitType shrink `required` / `properties`.
// ---------------------------------------------------------------------------

#[test]
fn partial_type_schema_marks_every_field_optional() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(UpdateUserDto)).expect("schema serializes");
    assert_eq!(schema["type"], "object");
    // Required list either empty (everything nullable) or absent — never
    // includes any of the field names.
    if let Some(req) = schema.get("required") {
        let req = req.as_array().expect("required is array when present");
        assert!(
            req.is_empty(),
            "PartialType must drop all required entries; got {req:?}"
        );
    }
    // Properties should still contain every field — just nullable now.
    let props = schema["properties"].as_object().expect("properties");
    for k in ["email", "name", "age", "nickname"] {
        assert!(props.contains_key(k), "{k} still in schema properties");
    }
}

#[test]
fn omit_type_schema_drops_named_field() {
    let schema: Value = serde_json::to_value(schemars::schema_for!(CreateUserNoNicknameDto))
        .expect("schema serializes");
    let props = schema["properties"].as_object().expect("properties");
    assert!(
        !props.contains_key("nickname"),
        "nickname dropped from schema"
    );
    assert!(props.contains_key("email"));
    assert!(props.contains_key("name"));
    assert!(props.contains_key("age"));
}

#[test]
fn pick_type_schema_keeps_only_named_fields() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(UserIdentityDto)).expect("schema serializes");
    let props = schema["properties"].as_object().expect("properties");
    let mut keys: Vec<_> = props.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["email", "name"],
        "only picked fields in the reflected schema"
    );
}

// ---------------------------------------------------------------------------
// `#[intersection_type]` — NestJS IntersectionType(A, B)
// ---------------------------------------------------------------------------

#[dto(allow_unknown_fields)]
struct IdentDto {
    #[IsEmail]
    email: String,
}

#[dto(allow_unknown_fields)]
struct ProfileDto {
    #[MinLength(2)]
    name: String,
}

#[nestrs::intersection_type]
struct CreateUserMergedDto {
    ident: IdentDto,
    profile: ProfileDto,
}

#[test]
fn intersection_type_deserializes_flat_json() {
    let dto: CreateUserMergedDto =
        serde_json::from_str(r#"{"email":"ada@example.com","name":"Ada"}"#)
            .expect("flat JSON deserializes into flattened parents");
    assert_eq!(dto.ident.email, "ada@example.com");
    assert_eq!(dto.profile.name, "Ada");
}

#[test]
fn intersection_type_serializes_flat_json() {
    let dto = CreateUserMergedDto {
        ident: IdentDto {
            email: "ada@example.com".to_string(),
        },
        profile: ProfileDto {
            name: "Ada".to_string(),
        },
    };
    let json = serde_json::to_value(&dto).expect("serializes");
    let obj = json.as_object().expect("object");
    let mut keys: Vec<_> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["email", "name"]);
}

#[test]
fn intersection_type_validates_nested_parents() {
    let dto: CreateUserMergedDto = serde_json::from_str(r#"{"email":"not-an-email","name":"Ada"}"#)
        .expect("shape is valid even when email fails IsEmail");
    let err = dto
        .validate()
        .expect_err("bad email must fail nested Validate");
    // Nested `#[validate(nested)]` reports `ValidationErrorsKind::Struct`,
    // which `field_errors()` filters out. Look at the full map instead.
    let errors = err.errors();
    assert!(
        errors.contains_key("ident"),
        "nested IsEmail should fire on the ident parent; got {:?}",
        errors.keys().collect::<Vec<_>>()
    );
}
