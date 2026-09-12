//! Production/security audit 2026-09-10 (macros): `#[dto]` validation markers
//! were silent no-ops. `#[IsUUID]` on a `String` field now emits a real runtime
//! UUID check (`nestrs::is_uuid` via `#[validate(custom(...))]`), and the
//! type-enforced markers (`IsString`, `IsBoolean`, `IsInt`, `IsNumber`) reject
//! marker/type contradictions at compile time instead of silently doing
//! nothing.

use nestrs::prelude::*;
use validator::Validate;

#[dto]
struct UserDto {
    #[IsUUID]
    id: String,
    #[IsOptional]
    #[IsUUID]
    parent_id: Option<String>,
    #[IsString]
    name: String,
    #[IsBoolean]
    active: bool,
    #[IsInt]
    age: i32,
    #[IsNumber]
    score: f64,
}

fn valid_dto() -> UserDto {
    UserDto {
        id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        parent_id: Some("550E8400-E29B-41D4-A716-446655440000".to_string()),
        name: "ada".to_string(),
        active: true,
        age: 37,
        score: 9.5,
    }
}

#[test]
fn is_uuid_accepts_canonical_uuid_v4() {
    valid_dto().validate().expect("canonical UUID passes");
}

#[test]
fn is_uuid_accepts_nil_uuid() {
    let mut dto = valid_dto();
    dto.id = "00000000-0000-0000-0000-000000000000".to_string();
    dto.validate().expect("nil UUID passes (IsUUID \"all\")");
}

#[test]
fn is_uuid_rejects_non_uuid_string() {
    let mut dto = valid_dto();
    dto.id = "not-a-uuid".to_string();
    let err = dto.validate().expect_err("garbage id must fail");
    let field_errors = err.field_errors();
    let field = field_errors
        .get("id")
        .expect("error attributed to the `id` field");
    assert!(
        field.iter().any(|ve| ve.code == "isUuid"),
        "expected an isUuid constraint, got {field:?}"
    );
}

#[test]
fn is_uuid_rejects_wrong_group_lengths() {
    for bad in [
        "550e8400e29b-41d4-a716-446655440000",     // missing hyphen
        "550e840-e29b-41d4-a716-4466554400000",    // 35 chars
        "550e8400-e29b-41d4-a716-446655440000\n ", // trailing junk
        "g50e8400-e29b-41d4-a716-446655440000",    // non-hex digit
    ] {
        let mut dto = valid_dto();
        dto.id = bad.to_string();
        assert!(
            dto.validate().is_err(),
            "`{bad}` should not pass the 8-4-4-4-12 hex check"
        );
    }
}

#[test]
fn is_uuid_skips_none_option_field() {
    let mut dto = valid_dto();
    dto.parent_id = None;
    dto.validate()
        .expect("None optional field skips validation");
}

#[test]
fn is_uuid_validates_some_option_field() {
    let mut dto = valid_dto();
    dto.parent_id = Some("definitely-not-a-uuid".to_string());
    let err = dto.validate().expect_err("Some(garbage) must fail");
    assert!(
        err.field_errors().contains_key("parent_id"),
        "error attributed to `parent_id`, got {:?}",
        err.field_errors().keys().collect::<Vec<_>>()
    );
}

#[test]
fn nestrs_is_uuid_helper_matches_class_validator_all() {
    // Direct helper checks — the same fn the macro wires in.
    assert!(nestrs::is_uuid("550e8400-e29b-41d4-a716-446655440000").is_ok());
    assert!(nestrs::is_uuid("6ba7b810-9dad-11d1-80b4-00c04fd430c8").is_ok()); // v1
    assert!(nestrs::is_uuid("6ba7b814-9dad-11d1-80b4-00c04fd430c8").is_ok()); // v1 variant
    assert!(nestrs::is_uuid("").is_err());
    assert!(nestrs::is_uuid("urn:uuid:550e8400-e29b-41d4-a716-446655440000").is_err()); // URN form rejected, like class-validator
    assert!(nestrs::is_uuid("{550e8400-e29b-41d4-a716-446655440000}").is_err());
    // braced form rejected
}
