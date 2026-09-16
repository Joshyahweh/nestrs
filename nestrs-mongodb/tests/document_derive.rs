//! Wave 7.6 Phase C — `#[derive(nestrs_mongodb::Document)]` smoke test.
//!
//! The compile-time checks below are the real test: if the derive
//! doesn't emit `impl Document for User`, these `T::collection_name()`
//! calls fail to compile. The runtime assertions verify the default
//! snake_case-plural fallback (`User` → `"users"`).

use bson::oid::ObjectId;
use nestrs_mongodb::Document;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Document)]
#[schema(collection = "users", timestamps)]
struct User {
    #[serde(skip_serializing_if = "Option::is_none")]
    _id: Option<ObjectId>,
    #[prop(rename = "email_address", unique)]
    email: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Document)]
struct BlogPost {
    #[serde(skip_serializing_if = "Option::is_none")]
    _id: Option<ObjectId>,
    title: String,
}

#[test]
fn schema_attribute_overrides_collection_name() {
    assert_eq!(User::collection_name(), "users");
}

#[test]
fn default_collection_name_is_snake_case_plural() {
    // `BlogPost` has no `#[schema(collection = …)]` — the derive should
    // fall back to snake_case + trailing "s".
    assert_eq!(BlogPost::collection_name(), "blog_posts");
}

#[test]
fn prop_attribute_parses_without_error() {
    // The derive parses every `#[prop(...)]` attribute up front; if
    // `rename` / `unique` weren't valid keys this file would fail to
    // compile. Just constructing the type is enough.
    let u = User {
        _id: None,
        email: "ada@example.com".into(),
        name: "Ada".into(),
    };
    assert_eq!(u.email, "ada@example.com");
}

#[test]
fn to_bson_round_trip_works() {
    let u = User {
        _id: Some(ObjectId::new()),
        email: "ada@example.com".into(),
        name: "Ada".into(),
    };
    let doc = u.to_bson().expect("serializes");
    assert_eq!(doc.get_str("email").unwrap(), "ada@example.com");
    assert_eq!(doc.get_str("name").unwrap(), "Ada");
}