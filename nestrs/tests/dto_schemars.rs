//! Wave 3F — `#[dto]` schemars reflection + validator 0.21 compatibility.
//!
//! `#[dto]` derives `schemars::JsonSchema` alongside `serde` +
//! `validator::Validate`; these tests assert the reflected schema
//! (round-trip, serde renames, nested `$ref` chains) and that the schema
//! flows into `nestrs-openapi`'s `components.schemas`.

use nestrs::prelude::*;
use nestrs::schemars;
use serde_json::Value;
// In scope at module level: `#[dto]`'s `#[validate(nested)]` codegen calls
// `.validate()` on nested types, which requires the trait to be in scope.
use validator::Validate;

#[dto]
struct CreateUserDto {
    #[IsEmail]
    email: String,
    #[MinLength(2)]
    name: String,
    age: i32,
}

#[dto]
struct UpdateUserDto {
    #[serde(rename = "userName")]
    #[MinLength(2)]
    name: String,
}

#[dto]
struct NestedInnerDto {
    #[IsUUID]
    id: String,
}

#[dto]
struct NestedOuterDto {
    inner: NestedInnerDto,
    #[ValidateNested]
    tagged: NestedInnerDto,
}

#[dto(allow_unknown_fields)]
struct LooseDto {
    anything: String,
}

// ---------------------------------------------------------------------------
// schema_for! round-trip
// ---------------------------------------------------------------------------

#[test]
fn dto_struct_round_trips_through_schema_for() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(CreateUserDto)).expect("schema serializes");
    assert_eq!(schema["type"], "object");
    let props = schema["properties"].as_object().expect("properties");
    let mut keys: Vec<_> = props.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["age", "email", "name"],
        "all dto fields reflected"
    );
    let mut required: Vec<_> = schema["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|v| v.as_str().expect("string").to_string())
        .collect();
    required.sort();
    assert_eq!(required, vec!["age", "email", "name"]);
    assert_eq!(schema["properties"]["email"]["type"], "string");
    assert_eq!(schema["properties"]["age"]["type"], "integer");
}

#[test]
fn serde_rename_is_reflected_in_schema() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(UpdateUserDto)).expect("schema serializes");
    let props = schema["properties"].as_object().expect("properties");
    assert!(
        props.contains_key("userName") && !props.contains_key("name"),
        "serde(rename) drives the schema key: {props:?}"
    );
}

#[test]
fn nested_dto_produces_ref_chain() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(NestedOuterDto)).expect("schema serializes");
    // Both nested fields point at the shared definition.
    assert!(schema["properties"]["inner"]["$ref"]
        .as_str()
        .expect("inner $ref")
        .contains("NestedInnerDto"));
    assert!(schema["properties"]["tagged"]["$ref"]
        .as_str()
        .expect("tagged $ref")
        .contains("NestedInnerDto"));
    // …and the definition itself lives in $defs.
    let defs = schema["$defs"].as_object().expect("$defs");
    assert!(defs.contains_key("NestedInnerDto"));
    assert_eq!(defs["NestedInnerDto"]["properties"]["id"]["type"], "string");
}

#[test]
fn allow_unknown_fields_variant_still_has_schema() {
    let schema: Value =
        serde_json::to_value(schemars::schema_for!(LooseDto)).expect("schema serializes");
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["anything"].is_object());
}

// ---------------------------------------------------------------------------
// validator 0.21 compatibility
// ---------------------------------------------------------------------------

#[test]
fn dto_validation_derive_works_under_validator_021() {
    let bad = CreateUserDto {
        email: "not-an-email".into(),
        name: "o".into(),
        age: 3,
    };
    assert!(
        bad.validate().is_err(),
        "IsEmail rejects a malformed address"
    );

    let good = CreateUserDto {
        email: "ada@example.com".into(),
        name: "ada".into(),
        age: 3,
    };
    assert!(good.validate().is_ok());
}

// ---------------------------------------------------------------------------
// OpenAPI components.schemas integration
// ---------------------------------------------------------------------------

#[cfg(feature = "openapi")]
#[tokio::test]
async fn openapi_components_schemas_carries_dto_schema() {
    use nestrs_openapi::{schema_entry, OpenApiOptions};
    use tower::util::ServiceExt;

    let expected: Value =
        serde_json::to_value(schemars::schema_for!(CreateUserDto)).expect("schema serializes");

    let options =
        OpenApiOptions::default().with_schemas([schema_entry::<CreateUserDto>("CreateUserDto")]);
    let router = nestrs_openapi::openapi_router(options);

    let res = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/openapi.json")
                .body(axum::body::Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");

    let bytes = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let doc: Value = serde_json::from_slice(&bytes).expect("json");

    assert_eq!(
        doc["components"]["schemas"]["CreateUserDto"], expected,
        "components.schemas entry is exactly the schemars output"
    );
}
