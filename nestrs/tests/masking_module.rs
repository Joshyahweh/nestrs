#![cfg(all(feature = "authz", feature = "authn"))]

//! `PolicyMaskingInterceptor` end-to-end tests over a real Axum router.
//! Covers Feature C (response masking across transports).

use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use axum::middleware::{from_fn, Next};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use nestrs::{json_response, mask_response, Ability, Action, MaskingConfig};
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
    assert!(v.get("body").is_none(), "runtime-built type name still masks");
    assert_eq!(v.get("title").and_then(|t| t.as_str()), Some("hi"));
}
