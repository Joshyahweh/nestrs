#![cfg(all(feature = "authz", feature = "authn"))]

//! `PolicyMaskingInterceptor` end-to-end tests over a real Axum router.
//! Covers Feature C (response masking across transports).

use axum::body::Body;
use axum::http::{header, Request as HttpRequest, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use nestrs::{json_response, mask_response, Ability, Action, Conditions, MaskingConfig};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::util::ServiceExt;

fn ability_with_post_fields() -> Arc<Ability> {
    // Principal can read `Post`, but only `id` and `title` — no `body`.
    Arc::new(
        Ability::builder()
            .can_on_fields(Action::Read, "Post", vec!["id".into(), "title".into()])
            .build(),
    )
}

fn ability_with_post_all_fields() -> Arc<Ability> {
    Arc::new(Ability::builder().can(Action::Read, "Post").build())
}

/// Stand-in for `install_policies_middleware` in tests: stashes the ability
/// into extensions and the ABILITY_SLOT.
async fn stash_ability_layer(
    axum::extract::State(ability): axum::extract::State<Arc<Ability>>,
    req: axum::extract::Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    parts.extensions.insert(ability.clone());
    let req = axum::extract::Request::from_parts(parts, body);
    nestrs::with_ability(ability, next.run(req)).await
}

#[tokio::test]
async fn masking_strips_field_when_principal_lacks_read_on_field() {
    let ability = ability_with_post_fields();
    let app = Router::new()
        .route(
            "/post",
            get(|| async {
                json_response(
                    StatusCode::OK,
                    json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" }),
                )
            }),
        )
        // Masking FIRST (applied last → runs first, wrapping the inner layers).
        .layer(nestrs::interceptor_layer!(nestrs::PolicyMaskingInterceptor))
        // THEN stash the ability so the masking interceptor can read it.
        .layer(from_fn(move |req, next| {
            let ab = ability.clone();
            async move { stash_ability_layer(axum::extract::State(ab), req, next).await }
        }));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .uri("/post")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
    assert!(v.get("body").is_none(), "body should be masked away");
    assert_eq!(v.get("id").and_then(|i| i.as_i64()), Some(1));
}

#[tokio::test]
async fn masking_keeps_field_when_principal_has_full_read() {
    let ability = ability_with_post_all_fields();
    let app = Router::new()
        .route(
            "/post",
            get(|| async {
                json_response(
                    StatusCode::OK,
                    json!({ "type": "Post", "id": 1, "title": "hi", "body": "ok" }),
                )
            }),
        )
        .layer(from_fn(move |req, next| {
            let ab = ability.clone();
            async move { stash_ability_layer(axum::extract::State(ab), req, next).await }
        }))
        .layer(nestrs::interceptor_layer!(nestrs::PolicyMaskingInterceptor));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .uri("/post")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.get("body").and_then(|t| t.as_str()), Some("ok"));
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}

#[tokio::test]
async fn masking_passes_through_body_without_type_marker() {
    // No `"type"` key in body → no masking decision → body unchanged.
    let ability = ability_with_post_fields();
    let app = Router::new()
        .route(
            "/list",
            get(|| async {
                json_response(
                    StatusCode::OK,
                    json!([{ "id": 1, "title": "hi" }, { "id": 2, "title": "x" }]),
                )
            }),
        )
        .layer(from_fn(move |req, next| {
            let ab = ability.clone();
            async move { stash_ability_layer(axum::extract::State(ab), req, next).await }
        }))
        .layer(nestrs::interceptor_layer!(nestrs::PolicyMaskingInterceptor));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .uri("/list")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.as_array().map(|a| a.len()), Some(2));
    // Unchanged: both items present with all fields.
    assert!(v[0].get("title").is_some());
}

#[tokio::test]
async fn masking_does_not_run_when_middleware_not_installed() {
    // No stash layer → no ability in extensions → pass-through.
    let app = Router::new()
        .route(
            "/post",
            get(|| async {
                json_response(
                    StatusCode::OK,
                    json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" }),
                )
            }),
        )
        .layer(nestrs::interceptor_layer!(nestrs::PolicyMaskingInterceptor));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .uri("/post")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    // Body was not masked, since there was no ability to read.
    assert_eq!(v.get("body").and_then(|t| t.as_str()), Some("secret"));
}

#[tokio::test]
async fn masking_unit_helper_strips_fields_directly() {
    // Direct unit-level call into `mask_response` so we don't have to spin
    // up a router for the simplest case.
    let ability = ability_with_post_fields();
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    assert_eq!(masked.status(), StatusCode::OK);
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(v.get("body").is_none());
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}

#[tokio::test]
async fn masking_recurses_into_arrays_of_objects() {
    let ability = ability_with_post_fields();
    let resp = json_response(
        StatusCode::OK,
        json!([
            { "type": "Post", "id": 1, "title": "a", "body": "x" },
            { "type": "Post", "id": 2, "title": "b", "body": "y" }
        ]),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    let items = v.as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert!(item.get("body").is_none());
        assert!(item.get("title").is_some());
    }
}

#[tokio::test]
async fn masking_walks_items_inside_an_unreadable_list_wrapper() {
    // A `PostList` the principal cannot read is not a reason to drop the
    // Posts inside it. Scalar fields on the wrapper go; the array stays
    // and each element is field-masked.
    let ability = ability_with_post_fields();
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "PostList",
            "total": 2,
            "items": [
                { "type": "Post", "id": 1, "title": "a", "body": "x" },
                { "type": "Post", "id": 2, "title": "b", "body": "y" }
            ]
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("PostList"));
    assert!(
        v.get("total").is_none(),
        "scalar fields of an unreadable wrapper are dropped, got {v}"
    );
    let items = v
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items array");
    assert_eq!(items.len(), 2);
    for item in items {
        assert!(
            item.get("body").is_none(),
            "body should be masked, got {item}"
        );
        assert!(
            item.get("title").is_some(),
            "title should remain, got {item}"
        );
        assert!(item.get("id").is_some(), "id should remain, got {item}");
    }
}

#[tokio::test]
async fn masking_still_strips_fields_when_body_is_exactly_the_cap() {
    let ability = ability_with_post_fields();
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" }),
    );
    let raw = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": "Post", "id": 1, "title": "hi", "body": "secret" }),
    );
    let cfg = MaskingConfig {
        max_body_bytes: raw.len(),
    };
    let masked = mask_response(resp, &ability, cfg).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v.get("body").is_none(),
        "a body that fills the cap must still be masked, got {v}"
    );
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}

#[tokio::test]
async fn masking_strips_fields_when_principal_cannot_read_the_type() {
    // `allowed_fields` is `None` both when no rule matches and when a
    // grant has no field list. The no-rule case used to return the object
    // intact, so a `User` payload on a principal who can only read `Post`
    // kept `ssn`.
    let ability = ability_with_post_fields();
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "User",
            "id": 7,
            "email": "ada@example.com",
            "ssn": "123-45-6789"
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("User"));
    assert!(
        v.get("ssn").is_none() && v.get("email").is_none() && v.get("id").is_none(),
        "an unreadable subject must not keep its fields, got {v}"
    );
}

#[tokio::test]
async fn masking_strips_nested_object_when_its_type_is_not_readable() {
    let ability = Arc::new(
        Ability::builder()
            .can_on_fields(
                Action::Read,
                "Post",
                vec!["type".into(), "id".into(), "title".into(), "author".into()],
            )
            .build(),
    );
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "Post",
            "id": 1,
            "title": "hi",
            "body": "secret",
            "author": { "type": "User", "name": "ada", "ssn": "secret" }
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v.get("body").is_none(),
        "post body is not in the allow-list"
    );
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
    let author = v.get("author").expect("author is allow-listed");
    assert_eq!(author.get("type").and_then(|t| t.as_str()), Some("User"));
    assert!(
        author.get("ssn").is_none() && author.get("name").is_none(),
        "nested unreadable subject must not keep its fields, got {author}"
    );
}

#[tokio::test]
async fn masking_empty_ability_drops_fields_on_a_typed_object() {
    let ability = Arc::new(Ability::builder().build());
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": "Post", "id": 1, "body": "secret" }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v.as_object().map(|m| m.len()), Some(1));
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("Post"));
}

#[tokio::test]
async fn masking_masks_runtime_built_type_names_without_leaking() {
    // Audit regression: the walker used to Box::leak the response's "type"
    // value into a Subject::Type(&'static str) — one leaked allocation per
    // masked object per response, with attacker-influenceable content.
    // Subject::Type owns its name now; a runtime-built (non-'static) name
    // masks identically and is freed with the subject.
    let ability = ability_with_post_fields();
    let type_name = format!("{}{}", "Po", "st");
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": type_name, "id": 1, "title": "hi", "body": "secret" }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        v.get("body").is_none(),
        "runtime-built type name still masks"
    );
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}

#[tokio::test]
async fn masking_strips_an_instance_that_fails_its_condition() {
    // A conditional grant matches `Subject::Type` for every instance.
    // Judging the object as that type returned `ssn` for a hospital the
    // principal cannot read.
    let mut conds = Conditions::new();
    conds.insert("org".into(), json!("hospital-a"));
    let ability = Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "Patient", conds)
            .build(),
    );
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "Patient",
            "org": "hospital-b",
            "ssn": "123-45-6789"
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("Patient"));
    assert!(
        v.get("ssn").is_none() && v.get("org").is_none(),
        "a condition miss must not keep fields, got {v}"
    );
}

#[tokio::test]
async fn masking_keeps_fields_when_the_instance_matches_its_condition() {
    let mut conds = Conditions::new();
    conds.insert("org".into(), json!("hospital-a"));
    let ability = Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "Patient", conds)
            .build(),
    );
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "Patient",
            "org": "hospital-a",
            "ssn": "123-45-6789"
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert_eq!(v.get("ssn").and_then(|s| s.as_str()), Some("123-45-6789"));
    assert_eq!(v.get("org").and_then(|s| s.as_str()), Some("hospital-a"));
}

#[tokio::test]
async fn masking_ignores_an_attributes_object_that_spoofs_the_condition() {
    let mut conds = Conditions::new();
    conds.insert("org".into(), json!("hospital-a"));
    let ability = Arc::new(
        Ability::builder()
            .can_with_conditions(Action::Read, "Patient", conds)
            .build(),
    );
    let resp = json_response(
        StatusCode::OK,
        json!({
            "type": "Patient",
            "org": "hospital-b",
            "ssn": "123-45-6789",
            "attributes": { "org": "hospital-a" }
        }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert_eq!(v.as_object().map(|m| m.len()), Some(1));
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("Patient"));
    assert!(
        v.get("ssn").is_none() && v.get("attributes").is_none(),
        "a nested attributes object must not satisfy the condition, got {v}"
    );
}

#[tokio::test]
async fn masking_predicate_drops_fields_the_principal_does_not_own() {
    let ability = Arc::new(
        Ability::builder()
            .can_with_predicate(
                Action::Read,
                "Post",
                Vec::new(),
                |row: &Value, principal: &nestrs::policies::Principal| {
                    row.get("author").and_then(|a| a.as_str()) == Some(principal.subject.as_str())
                },
            )
            .build(),
    );
    let alice = Arc::new(nestrs::policies::Principal {
        subject: "alice".into(),
        roles: vec![],
        claims: json!({}),
    });
    let ability_for_alice = ability.clone();
    nestrs::with_principal(alice, async move {
        let other = json_response(
            StatusCode::OK,
            json!({ "type": "Post", "author": "bob", "body": "secret" }),
        );
        let masked = mask_response(other, &ability_for_alice, MaskingConfig::default()).await;
        let v = response_json(masked).await;
        assert!(
            v.get("body").is_none() && v.get("author").is_none(),
            "a rejected predicate must not keep fields, got {v}"
        );
        assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("Post"));

        let own = json_response(
            StatusCode::OK,
            json!({ "type": "Post", "author": "alice", "body": "hello" }),
        );
        let masked = mask_response(own, &ability_for_alice, MaskingConfig::default()).await;
        let v = response_json(masked).await;
        assert_eq!(v.get("body").and_then(|b| b.as_str()), Some("hello"));
    })
    .await;
}

#[tokio::test]
async fn masking_predicate_drops_fields_when_no_principal_is_in_scope() {
    let ability = Arc::new(
        Ability::builder()
            .can_with_predicate(
                Action::Read,
                "Post",
                Vec::new(),
                |row: &Value, principal: &nestrs::policies::Principal| {
                    row.get("author").and_then(|a| a.as_str()) == Some(principal.subject.as_str())
                },
            )
            .build(),
    );
    let resp = json_response(
        StatusCode::OK,
        json!({ "type": "Post", "author": "alice", "body": "secret" }),
    );
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert_eq!(v.as_object().map(|m| m.len()), Some(1));
    assert_eq!(v.get("type").and_then(|t| t.as_str()), Some("Post"));
}

#[tokio::test]
async fn masking_withholds_json_the_parser_rejects() {
    // Trailing bytes fail `serde_json::from_slice`. Returning the original
    // body skipped field stripping; the secret stayed in the payload.
    let ability = ability_with_post_fields();
    let raw = br#"{"type":"Post","id":1,"title":"hi","body":"secret"}x"#;
    assert!(serde_json::from_slice::<Value>(raw).is_err());
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, raw.len().to_string())
        .body(Body::from(raw.to_vec()))
        .unwrap();
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    assert_eq!(masked.status(), StatusCode::OK);
    assert!(masked.headers().get(header::CONTENT_LENGTH).is_none());
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        body.is_empty(),
        "unparseable JSON must be withheld, got {}",
        String::from_utf8_lossy(&body)
    );
}

#[tokio::test]
async fn masking_withholds_json_deeper_than_the_parser_accepts() {
    // `from_slice` stops at 128 levels. A sibling field on a deeper
    // document used to be returned intact. The fixture is built as text
    // so constructing it does not recurse.
    let ability = ability_with_post_fields();
    let mut tree = "\"leaf\"".to_string();
    for _ in 0..200 {
        tree = format!("[{tree}]");
    }
    let raw = format!(r#"{{"type":"Post","title":"hi","body":"secret-body","tree":{tree}}}"#)
        .into_bytes();
    assert!(
        serde_json::from_slice::<Value>(&raw).is_err(),
        "the fixture must exceed the parser depth limit"
    );
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, raw.len().to_string())
        .body(Body::from(raw))
        .unwrap();
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    assert!(masked.headers().get(header::CONTENT_LENGTH).is_none());
    let body = axum::body::to_bytes(masked.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        !body
            .windows(b"secret-body".len())
            .any(|w| w == b"secret-body"),
        "a body past the parser depth limit must not be returned unmasked"
    );
    assert!(body.is_empty());
}

#[tokio::test]
async fn masking_matches_json_content_type_case_insensitively() {
    let ability = ability_with_post_fields();
    let raw = serde_json::to_vec(&json!({
        "type": "Post",
        "id": 1,
        "title": "hi",
        "body": "secret"
    }))
    .unwrap();
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "Application/JSON; charset=utf-8")
        .body(Body::from(raw))
        .unwrap();
    let masked = mask_response(resp, &ability, MaskingConfig::default()).await;
    let v = response_json(masked).await;
    assert!(v.get("body").is_none(), "got {v}");
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}

async fn response_json(response: Response) -> Value {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}
