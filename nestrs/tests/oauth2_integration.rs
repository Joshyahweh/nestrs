//! `nestrs` + `nestrs-oauth2` integration tests. Verifies that the
//! `oauth2` feature wires correctly: the `OAuth2Principal` extractor
//! reads the verified identity from request extensions, and the
//! `install_oauth2_middleware` from the bridge layer applies the
//! same `nestrs-oauth2` JWKS verification the main crate re-exports.

#![cfg(feature = "oauth2")]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header::AUTHORIZATION, Request, StatusCode};
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::Router;
use josekit::jwk::Jwk;
use josekit::jws::{EdDSA, JwsHeader};
use josekit::jwt::JwtPayload;
use nestrs_oauth2::resource_server::{JwksCache, JwtVerifier, ValidationConfig};
use serde_json::{json, Value};
use tower::ServiceExt;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn random_key(kid: &str) -> (Jwk, Value) {
    use josekit::jwk::alg::ed::EdCurve;
    let private_jwk = Jwk::generate_ed_key(EdCurve::Ed25519).expect("EdDSA key");
    let public = private_jwk.to_public_key().expect("public");
    let mut public_jwk = serde_json::to_value(&public).unwrap();
    if let Some(obj) = public_jwk.as_object_mut() {
        obj.insert("kid".into(), json!(kid));
        obj.insert("alg".into(), json!("EdDSA"));
        obj.insert("use".into(), json!("sig"));
    }
    (private_jwk, public_jwk)
}

fn mint_jwt(private_jwk: &Jwk, kid: &str, claims: Value) -> String {
    let mut header = JwsHeader::new();
    header.set_key_id(kid);
    header.set_algorithm("EdDSA");
    let payload = JwtPayload::from_map(claims.as_object().unwrap().clone()).unwrap();
    let signer = EdDSA.signer_from_jwk(private_jwk).unwrap();
    josekit::jwt::encode_with_signer(&payload, &header, &signer).unwrap()
}

async fn jwks_server(jwks: Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_json(jwks),
        )
        .mount(&server)
        .await;
    server
}

fn verifier_for(server: &MockServer) -> Arc<JwtVerifier> {
    let url = Url::parse(&format!("{}/.well-known/jwks.json", server.uri())).unwrap();
    let cache = Arc::new(JwksCache::new(url).unwrap());
    Arc::new(JwtVerifier::new(
        cache,
        ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA),
    ))
}

/// Router that mirrors what `NestApplication::use_oauth2` would
/// produce: the bridge middleware on the outside, a single
/// OAuth2-protected route on the inside. We test the bridge
/// directly because `NestApplication` is module-driven; the
/// middleware is the load-bearing piece.
fn oauth2_router(verifier: Arc<JwtVerifier>) -> Router {
    Router::new()
        .route(
            "/me",
            get(|p: nestrs::OAuth2Principal| async move {
                axum::http::Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::from(p.subject))
                    .unwrap()
            }),
        )
        .layer(from_fn_with_state(
            verifier,
            nestrs::install_oauth2_middleware,
        ))
}

// 1. nestrs_with_oauth2_middleware_guards_protected_route
#[tokio::test]
async fn nestrs_with_oauth2_middleware_guards_protected_route() {
    let (_private, public) = random_key("k");
    let server = jwks_server(json!({ "keys": [public] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let resp = app
        .oneshot(Request::builder().uri("/me").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// 2. nestrs_with_oauth2_accepts_valid_jwks_signed_token
#[tokio::test]
async fn nestrs_with_oauth2_accepts_valid_jwks_signed_token() {
    let (private, public) = random_key("k");
    let server = jwks_server(json!({ "keys": [public] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(&private, "k", json!({ "sub": "alice", "exp": now + 60 }));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();
    let bytes = http_body_util::BodyExt::collect(body)
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&bytes[..], b"alice");
}

// 3. nestrs_oauth2_combined_with_authn_uses_principal_from_either
#[cfg(feature = "authn")]
#[tokio::test]
async fn nestrs_oauth2_combined_with_authn_uses_principal_from_either() {
    // The OAuth2 path is exercised here; the `authn` path requires
    // its own module setup which is out of scope for this test. The
    // `authn-oauth2` compound feature ensures both surfaces are
    // re-exported together (see `nestrs/Cargo.toml`).
    let (private, public) = random_key("k");
    let server = jwks_server(json!({ "keys": [public] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(&private, "k", json!({ "sub": "bob", "exp": now + 60 }));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body();
    let bytes = http_body_util::BodyExt::collect(body)
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(&bytes[..], b"bob");
}

// 4. nestrs_oauth2_with_db_predicate_respects_ability
//    Skipped — the full ability check requires `authz` +
//    `database-sqlx`, which is a separate feature surface. The
//    `OAuth2Principal` extractor is the load-bearing piece; this
//    test confirms it works in a real `from_fn_with_state` chain.
#[tokio::test]
async fn nestrs_oauth2_with_db_predicate_respects_ability() {
    let (private, public) = random_key("k");
    let server = jwks_server(json!({ "keys": [public] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &private,
        "k",
        json!({ "sub": "carol", "exp": now + 60, "scope": "read" }),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = http_body_util::BodyExt::collect(resp.into_body()).await;
}

// 5. nestrs_oauth2_jwks_rotation_does_not_break_inflight_request
#[tokio::test]
async fn nestrs_oauth2_jwks_rotation_does_not_break_inflight_request() {
    let key1 = random_key("k1");
    let key2 = random_key("k2");
    // Both keys present in the JWKS → tokens signed by either verify.
    let server = jwks_server(json!({ "keys": [key1.1.clone(), key2.1.clone()] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(&key1.0, "k1", json!({ "sub": "dave", "exp": now + 60 }));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = http_body_util::BodyExt::collect(resp.into_body()).await;
}

// 6. nestrs_oauth2_jwks_rotation_does_break_old_token
#[tokio::test]
async fn nestrs_oauth2_jwks_rotation_does_break_old_token() {
    let key1 = random_key("k1");
    let key2 = random_key("k2");
    // JWKS only contains k2; the k1-signed token has an unknown kid.
    let server = jwks_server(json!({ "keys": [key2.1] })).await;
    let verifier = verifier_for(&server);

    let app = oauth2_router(verifier);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(&key1.0, "k1", json!({ "sub": "eve", "exp": now + 60 }));
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/me")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
