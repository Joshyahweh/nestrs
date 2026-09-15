//! Integration tests for the OAuth2 **authorization server** (Wave 6.1).
//!
//! Every security-critical transition has a test: PKCE enforcement,
//! single-use codes (replay → family revocation), refresh rotation
//! (reuse → family + access-JWT revocation), open-redirect safety,
//! client authentication, introspection, and the JWKS round-trip
//! proving issued tokens verify with the crate's own resource server.

#![cfg(feature = "authorization-server")]

use std::sync::Arc;

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use http_body_util::BodyExt;
use nestrs_oauth2::authorization_server::model::{GrantType, OAuth2ClientRecord};
use nestrs_oauth2::authorization_server::routes;
use nestrs_oauth2::authorization_server::service::{AuthorizationServer, ResourceOwnerSource};
use nestrs_oauth2::authorization_server::{AuthorizationServerConfig, AuthorizationServerStores};
use nestrs_oauth2::{JwtVerifier, ValidationConfig};
use pkcs8::{EncodePrivateKey, LineEnding};
use rand::RngCore as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use url::form_urlencoded::Serializer;

use axum::body::Body;
use axum::http::Request;
use axum::response::Response;
use axum::Router;

const ISSUER: &str = "https://idp.example.test";
const CONFIDENTIAL_ID: &str = "confidential-web";
const CONFIDENTIAL_SECRET: &str = "s3cret-confidential-value";
const CONFIDENTIAL_REDIRECT: &str = "https://client.example.test/cb";
const PUBLIC_ID: &str = "public-spa";
const PUBLIC_REDIRECT: &str = "https://spa.example.test/cb";
/// Deterministic 43-char verifier — PKCE randomness is the client's
/// job, and codes are single-use, so a fixed verifier is fine here.
const VERIFIER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BAD_VERIFIER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// ---------- helpers ----------

fn signing_key_pem() -> String {
    // ed25519-dalek 2 has no rand-based `generate` without the
    // `rand_core` feature — pull 32 random bytes and build from bytes.
    let mut secret = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut secret);
    ed25519_dalek::SigningKey::from_bytes(&secret)
        .to_pkcs8_pem(LineEnding::LF)
        .unwrap()
        .to_string()
}

fn confidential_client() -> OAuth2ClientRecord {
    OAuth2ClientRecord::confidential(
        CONFIDENTIAL_ID,
        CONFIDENTIAL_SECRET,
        vec![CONFIDENTIAL_REDIRECT.into()],
        vec![
            GrantType::AuthorizationCode,
            GrantType::RefreshToken,
            GrantType::ClientCredentials,
        ],
        vec!["read".into(), "write".into()],
    )
    .unwrap()
}

fn public_client() -> OAuth2ClientRecord {
    OAuth2ClientRecord::public(
        PUBLIC_ID,
        vec![PUBLIC_REDIRECT.into()],
        vec![GrantType::AuthorizationCode, GrantType::RefreshToken],
        vec!["read".into()],
    )
    .unwrap()
}

/// Test router. `owner` = the resource-owner subject the session
/// middleware reports (`None` = unauthenticated); `require_pkce` mirrors
/// `AuthorizationServerConfig::require_pkce_for_confidential`.
fn test_router_with(owner: Option<&'static str>, require_pkce: bool) -> Router {
    let config = AuthorizationServerConfig::new(ISSUER, signing_key_pem(), "test-kid")
        .unwrap()
        .require_pkce_for_confidential(require_pkce);
    let resource_owner: ResourceOwnerSource =
        Arc::new(move |_parts: &axum::http::request::Parts| owner.map(str::to_string));
    let server = AuthorizationServer::new(
        config,
        AuthorizationServerStores::in_memory(vec![confidential_client(), public_client()]),
        resource_owner,
    )
    .unwrap();
    routes::router(Arc::new(server))
}

fn test_router() -> Router {
    test_router_with(Some("user-1"), true)
}

fn pkce_challenge() -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()))
}

fn urlencode(pairs: &[(&str, &str)]) -> String {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

async fn get(app: Router, uri: &str) -> Response {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn post_form(
    app: Router,
    uri: &str,
    headers: &[(&str, String)],
    form: &[(&str, &str)],
) -> Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded");
    for (name, value) in headers {
        builder = builder.header(*name, value.as_str());
    }
    app.oneshot(builder.body(Body::from(urlencode(form))).unwrap())
        .await
        .unwrap()
}

fn basic(id: &str, secret: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{id}:{secret}")))
}

async fn body_json(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn location(response: &Response) -> Option<String> {
    response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn location_param(url: &str, key: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn authorize_uri(client_id: &str, redirect: &str, scope: Option<&str>) -> String {
    let challenge = pkce_challenge();
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", "xyz-state"),
    ];
    if let Some(scope) = scope {
        pairs.push(("scope", scope));
    }
    format!("/oauth/authorize?{}", urlencode(&pairs))
}

/// Run `/authorize` → `/token` (PKCE) for `client_id` and return the
/// token-endpoint JSON body. Confidential clients authenticate with
/// Basic; public clients with the form `client_id` (no secret).
async fn full_pkce_flow(app: Router, client_id: &str, redirect: &str, scope: Option<&str>) -> Value {
    let response = get(app.clone(), &authorize_uri(client_id, redirect, scope)).await;
    assert_eq!(response.status(), 302, "authorize must redirect");
    let target = location(&response).expect("Location header");
    assert_eq!(
        location_param(&target, "state").as_deref(),
        Some("xyz-state"),
        "state must round-trip"
    );
    let code = location_param(&target, "code").expect("code in redirect");

    let form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", client_id),
        ("code", &code),
        ("redirect_uri", redirect),
        ("code_verifier", VERIFIER),
    ];
    let mut headers: Vec<(&str, String)> = Vec::new();
    if client_id == CONFIDENTIAL_ID {
        headers.push(("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET)));
    }
    let response = post_form(app, "/oauth/token", &headers, &form).await;
    let status = response.status();
    let body = body_json(response).await;
    assert_eq!(status, 200, "token exchange failed: {body:?}");
    body
}

/// Introspect a token using the confidential client's credentials.
async fn introspect(app: Router, token: &str) -> Value {
    let response = post_form(
        app,
        "/oauth/introspect",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[("token", token)],
    )
    .await;
    assert_eq!(response.status(), 200);
    body_json(response).await
}

// ---------- discovery + JWKS ----------

#[tokio::test]
async fn discovery_document_lists_all_endpoints() {
    let app = test_router();
    let response = get(app, "/.well-known/oauth-authorization-server").await;
    assert_eq!(response.status(), 200);
    let doc = body_json(response).await;
    assert_eq!(doc["issuer"], json!(ISSUER));
    assert_eq!(
        doc["authorization_endpoint"],
        json!("https://idp.example.test/oauth/authorize")
    );
    assert_eq!(
        doc["token_endpoint"],
        json!("https://idp.example.test/oauth/token")
    );
    assert_eq!(
        doc["introspection_endpoint"],
        json!("https://idp.example.test/oauth/introspect")
    );
    assert_eq!(
        doc["revocation_endpoint"],
        json!("https://idp.example.test/oauth/revoke")
    );
    assert_eq!(
        doc["jwks_uri"],
        json!("https://idp.example.test/.well-known/jwks.json")
    );
    assert_eq!(
        doc["grant_types_supported"],
        json!(["authorization_code", "refresh_token", "client_credentials"])
    );
    assert_eq!(doc["response_types_supported"], json!(["code"]));
    assert_eq!(
        doc["token_endpoint_auth_methods_supported"],
        json!(["client_secret_basic", "client_secret_post"])
    );
    assert_eq!(doc["code_challenge_methods_supported"], json!(["S256"]));
}

#[tokio::test]
async fn jwks_document_is_an_ed25519_okp_jwk() {
    let app = test_router();
    let response = get(app, "/.well-known/jwks.json").await;
    assert_eq!(response.status(), 200);
    let doc = body_json(response).await;
    let key = &doc["keys"][0];
    assert_eq!(key["kty"], json!("OKP"));
    assert_eq!(key["crv"], json!("Ed25519"));
    assert_eq!(key["alg"], json!("EdDSA"));
    assert_eq!(key["use"], json!("sig"));
    assert_eq!(key["kid"], json!("test-kid"));
    let x = key["x"].as_str().unwrap();
    assert_eq!(x.len(), 43, "raw Ed25519 key is 32 bytes → 43 b64url chars");
    assert!(URL_SAFE_NO_PAD.decode(x).is_ok(), "x must be base64url");
}

// ---------- client_credentials ----------

#[tokio::test]
async fn client_credentials_issues_jwt_with_expected_claims() {
    let app = test_router();
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[("grant_type", "client_credentials")],
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("cache-control").unwrap(),
        "no-store",
        "token responses must not be cached"
    );
    let body = body_json(response).await;
    assert_eq!(body["token_type"], json!("Bearer"));
    assert_eq!(body["expires_in"], json!(900));
    assert_eq!(body["scope"], json!("read write"), "default = full client scope");
    assert!(
        body.get("refresh_token").is_none(),
        "client_credentials must not issue a refresh token"
    );
    let access = body["access_token"].as_str().unwrap().to_string();

    // The introspection endpoint is the RFC-specified way to read the
    // token state back — it self-verifies signature + expiry. Same app
    // instance: the token was signed and stored by *this* router.
    let info = introspect(app, &access).await;
    assert_eq!(info["active"], json!(true));
    assert_eq!(info["token_type"], json!("Bearer"));
    assert_eq!(info["sub"], json!(CONFIDENTIAL_ID), "the client is the subject");
    assert_eq!(info["client_id"], json!(CONFIDENTIAL_ID));
    assert_eq!(info["iss"], json!(ISSUER));
    assert_eq!(info["scope"], json!("read write"));
    assert!(info["jti"].is_string() && !info["jti"].as_str().unwrap().is_empty());
    assert!(info["aud"].is_string());
}

#[tokio::test]
async fn client_credentials_requires_confidential_client() {
    let app = test_router();
    // Public client, no secret — authenticates fine (that's the point of
    // public clients), but the grant is confidential-only.
    let response = post_form(
        app,
        "/oauth/token",
        &[],
        &[("grant_type", "client_credentials"), ("client_id", PUBLIC_ID)],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("unauthorized_client"));
}

// ---------- authorization-code + PKCE ----------

#[tokio::test]
async fn authorization_code_full_flow_with_pkce() {
    let app = test_router();
    let body = full_pkce_flow(app.clone(), PUBLIC_ID, PUBLIC_REDIRECT, None).await;
    assert_eq!(body["token_type"], json!("Bearer"));
    assert_eq!(body["scope"], json!("read"));
    assert!(body["refresh_token"].is_string(), "auth-code grants issue a refresh token");
    let access = body["access_token"].as_str().unwrap().to_string();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();

    // Same app instance: the token was signed and stored by *this*
    // router — a fresh one would have a different key and empty stores.
    let info = introspect(app.clone(), &access).await;
    assert_eq!(info["active"], json!(true));
    assert_eq!(info["sub"], json!("user-1"), "the resource owner, not the client");
    assert_eq!(info["client_id"], json!(PUBLIC_ID));
    assert_eq!(info["scope"], json!("read"));

    // Refresh tokens introspect too.
    let info = introspect(app, &refresh).await;
    assert_eq!(info["active"], json!(true));
    assert_eq!(info["token_type"], json!("refresh_token"));
    assert_eq!(info["client_id"], json!(PUBLIC_ID));
    assert_eq!(info["sub"], json!("user-1"));
}

#[tokio::test]
async fn authorize_without_resource_owner_returns_401() {
    let app = test_router_with(None, true);
    let response = get(app, &authorize_uri(PUBLIC_ID, PUBLIC_REDIRECT, None)).await;
    assert_eq!(response.status(), 401);
    assert!(location(&response).is_none(), "must not redirect without a user");
    assert_eq!(body_json(response).await["error"], json!("access_denied"));
}

#[tokio::test]
async fn public_client_without_pkce_is_rejected_via_redirect() {
    let app = test_router();
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", PUBLIC_ID),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("state", "st"),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 302, "client + redirect are valid → error goes via redirect");
    let target = location(&response).unwrap();
    assert_eq!(location_param(&target, "error").as_deref(), Some("invalid_request"));
    assert_eq!(location_param(&target, "state").as_deref(), Some("st"));
}

#[tokio::test]
async fn confidential_client_without_pkce_is_rejected_by_default() {
    let app = test_router();
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", CONFIDENTIAL_ID),
            ("redirect_uri", CONFIDENTIAL_REDIRECT),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 302);
    let target = location(&response).unwrap();
    assert_eq!(location_param(&target, "error").as_deref(), Some("invalid_request"));
}

#[tokio::test]
async fn relaxed_config_allows_confidential_without_pkce() {
    let app = test_router_with(Some("user-1"), false);
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", CONFIDENTIAL_ID),
            ("redirect_uri", CONFIDENTIAL_REDIRECT),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 302);
    let target = location(&response).unwrap();
    assert!(location_param(&target, "code").is_some(), "PKCE-off config must issue a code");
}

#[tokio::test]
async fn plain_pkce_method_is_unsupported() {
    let app = test_router();
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", PUBLIC_ID),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("code_challenge", &pkce_challenge()),
            ("code_challenge_method", "plain"),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 302);
    let target = location(&response).unwrap();
    assert_eq!(location_param(&target, "error").as_deref(), Some("invalid_request"));
}

#[tokio::test]
async fn wrong_pkce_verifier_burns_the_code() {
    let app = test_router();
    let response = get(app.clone(), &authorize_uri(PUBLIC_ID, PUBLIC_REDIRECT, None)).await;
    let code = location_param(&location(&response).unwrap(), "code").unwrap();

    // Wrong verifier → invalid_grant …
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC_ID),
            ("code", &code),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("code_verifier", BAD_VERIFIER),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));

    // … and the code is consumed: the CORRECT verifier cannot redeem it
    // afterwards (no verifier-probing oracle).
    let response = post_form(
        app,
        "/oauth/token",
        &[],
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC_ID),
            ("code", &code),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("code_verifier", VERIFIER),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));
}

#[tokio::test]
async fn code_replay_revokes_the_whole_family() {
    let app = test_router();
    let body = full_pkce_flow(app.clone(), PUBLIC_ID, PUBLIC_REDIRECT, None).await;
    let access = body["access_token"].as_str().unwrap().to_string();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();

    // Mint a second code from the same authorize request shape…
    let response = get(
        app.clone(),
        &authorize_uri(PUBLIC_ID, PUBLIC_REDIRECT, None),
    )
    .await;
    let code = location_param(&location(&response).unwrap(), "code").unwrap();
    // …redeem it once…
    let redeemed = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC_ID),
            ("code", &code),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("code_verifier", VERIFIER),
        ],
    )
    .await;
    assert_eq!(redeemed.status(), 200);
    let redeemed_body = body_json(redeemed).await;
    let second_refresh = redeemed_body["refresh_token"]
        .as_str()
        .expect("redeemed code issues a refresh token")
        .to_string();
    let second_access = redeemed_body["access_token"].as_str().unwrap().to_string();

    // …then REPLAY it: invalid_grant …
    let replay = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "authorization_code"),
            ("client_id", PUBLIC_ID),
            ("code", &code),
            ("redirect_uri", PUBLIC_REDIRECT),
            ("code_verifier", VERIFIER),
        ],
    )
    .await;
    assert_eq!(replay.status(), 400);
    assert_eq!(body_json(replay).await["error"], json!("invalid_grant"));

    // …and the family the replayed code seeded is dead: its refresh
    // token no longer works…
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &second_refresh),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));

    // …and neither does the FIRST family's refresh (each code = its own
    // family; the first flow's tokens are untouched).
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh),
        ],
    )
    .await;
    assert_eq!(response.status(), 200, "a different family must survive");

    // The REVOKED family's access JWT must introspect inactive — family
    // revocation kills outstanding access JWTs via jti…
    let info = introspect(app.clone(), &second_access).await;
    assert_eq!(
        info["active"],
        json!(false),
        "the replayed family's access JWT must be revoked"
    );

    // …while the first family's access JWT stays active.
    let info = introspect(app, &access).await;
    assert_eq!(info["active"], json!(true), "first family is untouched");
}

// ---------- refresh rotation + reuse detection ----------

#[tokio::test]
async fn refresh_rotation_and_reuse_revokes_the_whole_family() {
    let app = test_router();
    let body = full_pkce_flow(app.clone(), PUBLIC_ID, PUBLIC_REDIRECT, None).await;
    let access1 = body["access_token"].as_str().unwrap().to_string();
    let refresh1 = body["refresh_token"].as_str().unwrap().to_string();

    // Rotate: refresh1 → access2 + refresh2 (same family).
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh1),
        ],
    )
    .await;
    assert_eq!(response.status(), 200, "rotation must succeed");
    let body2 = body_json(response).await;
    assert_eq!(body2["scope"], json!("read"), "scope is preserved across rotation");
    let access2 = body2["access_token"].as_str().unwrap().to_string();
    let refresh2 = body2["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(refresh1, refresh2, "rotation must issue a NEW token");

    // The rotated token introspects inactive.
    let info = introspect(app.clone(), &refresh1).await;
    assert_eq!(
        info["active"],
        json!(false),
        "a rotated refresh token is no longer active"
    );

    // REUSE refresh1 (already rotated) → theft signal → the WHOLE
    // family dies.
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh1),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));

    // The successor is dead too (whole family revoked)…
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh2),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));

    // …and BOTH access JWTs are introspectable-inactive (jti revocation).
    let info = introspect(app.clone(), &access1).await;
    assert_eq!(info["active"], json!(false));
    let info = introspect(app, &access2).await;
    assert_eq!(info["active"], json!(false));
}

#[tokio::test]
async fn refresh_grant_can_narrow_but_not_widen_scope() {
    let app = test_router();
    let body = full_pkce_flow(
        app.clone(),
        CONFIDENTIAL_ID,
        CONFIDENTIAL_REDIRECT,
        Some("read write"),
    )
    .await;
    let refresh = body["refresh_token"].as_str().unwrap().to_string();
    let auth = [("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))];

    // Narrow to "read"…
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &auth,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("scope", "read"),
        ],
    )
    .await;
    assert_eq!(response.status(), 200, "narrowing scope must be allowed");
    let narrowed = body_json(response).await;
    assert_eq!(narrowed["scope"], json!("read"));
    let narrowed_refresh = narrowed["refresh_token"].as_str().unwrap().to_string();

    // …but widening the successor back to "read write" is invalid: a
    // refresh may never exceed the originally granted scope. Both
    // scopes are client-allowed, so this isolates the narrowing rule
    // from the client allow-list.
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &auth,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &narrowed_refresh),
            ("scope", "read write"),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_scope"));
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("originally granted"),
        "the description should name the rule: {:?}",
        body["error_description"]
    );
}

// ---------- open-redirect safety ----------

#[tokio::test]
async fn authorize_rejects_unregistered_redirect_uri_without_redirecting() {
    let app = test_router();
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", PUBLIC_ID),
            ("redirect_uri", "https://evil.test/cb"),
            ("code_challenge", &pkce_challenge()),
            ("code_challenge_method", "S256"),
        ])
    );
    let response = get(app, &uri).await;
    // Unverified client+redirect → DIRECT error, never a redirect: an
    // open /authorize must not become an open redirector.
    assert_eq!(response.status(), 400);
    assert!(
        location(&response).is_none(),
        "must never redirect to an unregistered URI"
    );
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_request"));
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("redirect_uri")
    );
}

#[tokio::test]
async fn authorize_rejects_unknown_client_without_redirecting() {
    let app = test_router();
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", "attacker"),
            ("redirect_uri", "https://evil.test/cb"),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 400);
    assert!(location(&response).is_none());
    assert_eq!(body_json(response).await["error"], json!("invalid_client"));
}

#[tokio::test]
async fn redirect_uris_match_byte_for_byte() {
    let app = test_router();
    // The registered URI plus an extra query param is a DIFFERENT URI —
    // exact match is what makes the allow-list meaningful.
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", PUBLIC_ID),
            ("redirect_uri", "https://spa.example.test/cb?x=1"),
            ("code_challenge", &pkce_challenge()),
            ("code_challenge_method", "S256"),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 400);
    assert!(location(&response).is_none());
}

#[tokio::test]
async fn a_client_cannot_use_another_clients_redirect_uri() {
    let app = test_router();
    // The redirect IS registered — but for a different client. Accepting
    // it would let a malicious client capture another's codes (the
    // OAuth "mix-up" attack).
    let uri = format!(
        "/oauth/authorize?{}",
        urlencode(&[
            ("response_type", "code"),
            ("client_id", PUBLIC_ID),
            ("redirect_uri", CONFIDENTIAL_REDIRECT),
            ("code_challenge", &pkce_challenge()),
            ("code_challenge_method", "S256"),
        ])
    );
    let response = get(app, &uri).await;
    assert_eq!(response.status(), 400);
    assert!(location(&response).is_none());
}

// ---------- client authentication ----------

#[tokio::test]
async fn token_endpoint_client_authentication_failures() {
    let app = test_router();

    // Wrong secret (Basic) → 401 + a Basic challenge (RFC 6750 style).
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, "wrong-secret"))],
        &[("grant_type", "client_credentials")],
    )
    .await;
    assert_eq!(response.status(), 401);
    assert_eq!(
        response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok()),
        Some("Basic realm=\"oauth2\"")
    );
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_client"));

    // Unknown client (Basic) → 401.
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic("ghost", CONFIDENTIAL_SECRET))],
        &[("grant_type", "client_credentials")],
    )
    .await;
    assert_eq!(response.status(), 401);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_client"));

    // Basic AND a form secret together → 400: RFC 6749 §2.3 forbids two
    // client authentication methods in one request.
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[
            ("grant_type", "client_credentials"),
            ("client_secret", CONFIDENTIAL_SECRET),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_request"));
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("multiple client authentication methods")
    );

    // Basic id ≠ form client_id → 400.
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[("grant_type", "client_credentials"), ("client_id", PUBLIC_ID)],
    )
    .await;
    assert_eq!(response.status(), 400);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_request"));
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("does not match")
    );

    // A public client must not authenticate with a secret.
    let response = post_form(
        app,
        "/oauth/token",
        &[("authorization", basic(PUBLIC_ID, "leaked-secret"))],
        &[("grant_type", "client_credentials")],
    )
    .await;
    assert_eq!(response.status(), 401);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_client"));
    assert!(
        body["error_description"]
            .as_str()
            .unwrap()
            .contains("public clients")
    );
}

// ---------- revocation (RFC 7009) ----------

#[tokio::test]
async fn revocation_kills_access_tokens_and_refresh_families() {
    let app = test_router();
    let body = full_pkce_flow(app.clone(), PUBLIC_ID, PUBLIC_REDIRECT, None).await;
    let access = body["access_token"].as_str().unwrap().to_string();
    let refresh = body["refresh_token"].as_str().unwrap().to_string();

    // Revoke the access JWT…
    let response = post_form(
        app.clone(),
        "/oauth/revoke",
        &[],
        &[("client_id", PUBLIC_ID), ("token", &access)],
    )
    .await;
    assert_eq!(response.status(), 200);

    // …it introspects inactive (jti on the revocation list)…
    let info = introspect(app.clone(), &access).await;
    assert_eq!(info["active"], json!(false));

    // …and revocation is idempotent (RFC 7009 §2.2: 200 regardless).
    let response = post_form(
        app.clone(),
        "/oauth/revoke",
        &[],
        &[("client_id", PUBLIC_ID), ("token", &access)],
    )
    .await;
    assert_eq!(response.status(), 200);

    // Revoke the refresh token → the whole family dies.
    let response = post_form(
        app.clone(),
        "/oauth/revoke",
        &[],
        &[("client_id", PUBLIC_ID), ("token", &refresh)],
    )
    .await;
    assert_eq!(response.status(), 200);

    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));

    // Family revocation also covers the (already individually revoked)
    // access JWT — still inactive.
    let info = introspect(app, &access).await;
    assert_eq!(info["active"], json!(false));
}

// ---------- refresh binding ----------

#[tokio::test]
async fn a_refresh_token_is_bound_to_its_client() {
    let app = test_router();
    let body = full_pkce_flow(app.clone(), PUBLIC_ID, PUBLIC_REDIRECT, None).await;
    let refresh = body["refresh_token"].as_str().unwrap().to_string();

    // Present the PUBLIC client's refresh token with a DIFFERENT
    // client's credentials → invalid_grant…
    let response = post_form(
        app.clone(),
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
    )
    .await;
    assert_eq!(response.status(), 400);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("invalid_grant"));

    // …and the mismatch revokes the whole family (token-theft posture):
    // even the rightful client can no longer use it.
    let response = post_form(
        app,
        "/oauth/token",
        &[],
        &[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_ID),
            ("refresh_token", &refresh),
        ],
    )
    .await;
    assert_eq!(response.status(), 400);
    assert_eq!(body_json(response).await["error"], json!("invalid_grant"));
}

// ---------- end-to-end: tokens verify against the resource server ----------

#[tokio::test]
async fn issued_tokens_verify_against_the_crate_resource_server() {
    // Real HTTP: serve the authorization server, point the crate's own
    // JWKS-based resource server at it, and verify a minted token —
    // the round trip that proves IdP and verifier interoperate.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let issuer = format!("http://{addr}");

    let config =
        AuthorizationServerConfig::new(&issuer, signing_key_pem(), "test-kid").unwrap();
    let server = AuthorizationServer::new(
        config,
        AuthorizationServerStores::in_memory(vec![confidential_client(), public_client()]),
        Arc::new(|_parts: &axum::http::request::Parts| Some("user-1".to_string())),
    )
    .unwrap();
    let app = routes::router(Arc::new(server));
    let app_for_mint = app.clone();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Machine grant: aud = client_id.
    let response = post_form(
        app_for_mint,
        "/oauth/token",
        &[("authorization", basic(CONFIDENTIAL_ID, CONFIDENTIAL_SECRET))],
        &[("grant_type", "client_credentials")],
    )
    .await;
    assert_eq!(response.status(), 200);
    let body = body_json(response).await;
    assert_eq!(body["token_type"], json!("Bearer"));
    let access = body["access_token"].as_str().unwrap().to_string();

    // The verifier fetches the JWKS over real HTTP and pins
    // issuer + audience (EdDSA is the default algorithm set).
    let verifier = JwtVerifier::from_url(
        url::Url::parse(&format!("{issuer}/.well-known/jwks.json")).unwrap(),
        ValidationConfig::default()
            .with_issuer(issuer.clone())
            .with_audience(CONFIDENTIAL_ID),
    )
    .await
    .unwrap();
    let data = verifier.verify(&access).await.unwrap();
    assert_eq!(data.claims["sub"], json!(CONFIDENTIAL_ID));
    assert_eq!(data.claims["iss"], json!(issuer));
    assert_eq!(data.claims["scope"], json!("read write"));
    assert_eq!(data.claims["client_id"], json!(CONFIDENTIAL_ID));

    // A tampered signature must fail.
    let parts: Vec<&str> = access.split('.').collect();
    let tampered = format!("{}.{}.{}", parts[0], parts[1], "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert!(verifier.verify(&tampered).await.is_err());
}
