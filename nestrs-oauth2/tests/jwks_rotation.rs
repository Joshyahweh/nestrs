//! JWKS-backed resource server tests. The verifier fetches a JWKS
//! from a `wiremock` server, looks up the `kid`, and validates the
//! JWT's claims. We mint test tokens with `josekit`'s built-in
//! Ed25519 keypair generation.

#![cfg(feature = "resource-server")]

use std::sync::Arc;
use std::time::Duration;

use josekit::jwk::Jwk;
use josekit::jws::{EdDSA, JwsHeader};
use josekit::jwt::JwtPayload;
use nestrs_oauth2::error::OAuth2Error;
use nestrs_oauth2::resource_server::{JwksCache, JwtVerifier, ValidationConfig};
use serde_json::{json, Value};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test fixture: a private JWK (kept for signing) + the public JWK
/// (handed to the verifier via the JWKS document).
struct TestKey {
    private_jwk: Jwk,
    public_jwk: Value,
}

fn random_key(kid: &str) -> TestKey {
    use josekit::jwk::alg::ed::EdCurve;
    let private_jwk = Jwk::generate_ed_key(EdCurve::Ed25519).expect("EdDSA key generation");
    let public = private_jwk.to_public_key().expect("public key");
    let mut public_jwk = serde_json::to_value(&public).unwrap();
    // Add `kid`, `alg`, `use` so the JWKS document looks like a real
    // one and the kid-based lookup works.
    if let Some(obj) = public_jwk.as_object_mut() {
        obj.insert("kid".into(), json!(kid));
        obj.insert("alg".into(), json!("EdDSA"));
        obj.insert("use".into(), json!("sig"));
    }
    TestKey {
        private_jwk,
        public_jwk,
    }
}

fn mint_jwt(private_jwk: &Jwk, kid: &str, claims: Value) -> String {
    let mut header = JwsHeader::new();
    header.set_key_id(kid);
    header.set_algorithm("EdDSA");
    let obj = claims.as_object().unwrap().clone();
    let payload = JwtPayload::from_map(obj).unwrap();
    let signer = EdDSA.signer_from_jwk(private_jwk).unwrap();
    josekit::jwt::encode_with_signer(&payload, &header, &signer).unwrap()
}

fn jwks_response(keys: Vec<Value>) -> Value {
    json!({ "keys": keys })
}

async fn jwks_mock_server(jwks: Value) -> MockServer {
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

fn jwks_url(server: &MockServer) -> Url {
    Url::parse(&format!("{}/.well-known/jwks.json", server.uri())).unwrap()
}

fn eddsa_verifier(cache: Arc<JwksCache>) -> JwtVerifier {
    JwtVerifier::new(cache, ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA))
}

// 1. verifies_token_signed_by_jwks_key
#[tokio::test]
async fn verifies_token_signed_by_jwks_key() {
    let kid = "key-1";
    let key = random_key(kid);
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = eddsa_verifier(cache);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        kid,
        json!({
            "sub": "user-1",
            "iss": "test",
            "aud": "test-aud",
            "iat": now,
            "exp": now + 60,
        }),
    );
    let data = verifier.verify(&token).await.unwrap();
    assert_eq!(
        data.claims.get("sub").and_then(|v| v.as_str()),
        Some("user-1")
    );
}

// 2. rejects_token_signed_by_unknown_kid
#[tokio::test]
async fn rejects_token_signed_by_unknown_kid() {
    let key = random_key("missing-kid");
    let server = jwks_mock_server(jwks_response(vec![])).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = eddsa_verifier(cache);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "missing-kid",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::UnknownKid(_)));
}

// 3. jwks_rotation_picks_up_new_key
#[tokio::test]
async fn jwks_rotation_picks_up_new_key() {
    let key1 = random_key("k1");
    let key2 = random_key("k2");
    let server = MockServer::start().await;
    // First call: only key1
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jwks_response(vec![key1.public_jwk.clone()])),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Second call: key1 + key2 (rotation)
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks_response(vec![
            key1.public_jwk.clone(),
            key2.public_jwk.clone(),
        ])))
        .mount(&server)
        .await;

    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = eddsa_verifier(cache);

    // Token signed by k1 → first call sees only k1, succeeds.
    let now = chrono::Utc::now().timestamp();
    let token1 = mint_jwt(
        &key1.private_jwk,
        "k1",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    verifier.verify(&token1).await.unwrap();

    // Token signed by k2 → unknown kid triggers refresh; the
    // second mock returns the rotated JWKS.
    let token2 = mint_jwt(
        &key2.private_jwk,
        "k2",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    let data = verifier.verify(&token2).await.unwrap();
    assert_eq!(data.claims.get("sub").and_then(|v| v.as_str()), Some("u"));
}

// 4. concurrent_first_fetches_coalesce
#[tokio::test]
async fn concurrent_first_fetches_coalesce() {
    let key = random_key("k");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jwks_response(vec![key.public_jwk.clone()])),
        )
        .expect(1) // coalesced into a single fetch
        .mount(&server)
        .await;

    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let v1 = Arc::new(eddsa_verifier(cache.clone()));
    let v2 = v1.clone();
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    let t1 = token.clone();
    let t2 = token.clone();
    let (r1, r2) = tokio::join!(v1.verify(&t1), v2.verify(&t2));
    r1.unwrap();
    r2.unwrap();
}

// 5. refresh_window_honoured
#[tokio::test]
async fn refresh_window_honoured() {
    let key = random_key("k");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(jwks_response(vec![key.public_jwk.clone()])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let cache = Arc::new(
        JwksCache::new(jwks_url(&server))
            .unwrap()
            .with_refresh_window(Duration::from_secs(60)),
    );
    let verifier = eddsa_verifier(cache.clone());
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    // First verify → triggers a fetch.
    verifier.verify(&token).await.unwrap();
    // Second verify within the refresh window → no new fetch.
    verifier.verify(&token).await.unwrap();
    // Sanity: the kid map is populated.
    assert!(cache.keys_snapshot().contains(&"k".to_string()));
}

// 6. validation_rejects_wrong_audience
#[tokio::test]
async fn validation_rejects_wrong_audience() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let validator = ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA).with_audience("expected");
    let verifier = JwtVerifier::new(cache, validator);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "aud": "wrong", "exp": now + 60 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::Validation(_)));
}

// 7. validation_rejects_wrong_issuer
#[tokio::test]
async fn validation_rejects_wrong_issuer() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let validator = ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA).with_issuer("expected");
    let verifier = JwtVerifier::new(cache, validator);
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "iss": "wrong", "exp": now + 60 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::Validation(_)));
}

// 8. validation_rejects_expired_token
#[tokio::test]
async fn validation_rejects_expired_token() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = JwtVerifier::new(
        cache,
        ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA).with_leeway(0),
    );
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(&key.private_jwk, "k", json!({ "exp": now - 60 }));
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::Validation(_)));
}

// 9. validation_rejects_nbf_in_future
#[tokio::test]
async fn validation_rejects_nbf_in_future() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = JwtVerifier::new(
        cache,
        ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA).with_leeway(0),
    );
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "nbf": now + 60, "exp": now + 120 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::Validation(_)));
}

// 10. validation_accepts_within_leeway
#[tokio::test]
async fn validation_accepts_within_leeway() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    let verifier = JwtVerifier::new(
        cache,
        ValidationConfig::new(jsonwebtoken::Algorithm::EdDSA).with_leeway(30),
    );
    let now = chrono::Utc::now().timestamp();
    // Token expired 10 seconds ago; 30s leeway accepts it.
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "exp": now - 10, "sub": "u" }),
    );
    verifier.verify(&token).await.unwrap();
}

// 11. algorithm_pinning_rejects_wrong_alg
#[tokio::test]
async fn algorithm_pinning_rejects_wrong_alg() {
    let key = random_key("k");
    let jwks = jwks_response(vec![key.public_jwk.clone()]);
    let server = jwks_mock_server(jwks).await;
    let cache = Arc::new(JwksCache::new(jwks_url(&server)).unwrap());
    // Pin to RS256 only; token is signed EdDSA → algorithm pin rejects.
    let verifier = JwtVerifier::new(cache, ValidationConfig::new(jsonwebtoken::Algorithm::RS256));
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    assert!(matches!(err, OAuth2Error::Validation(_)));
}

// 12. network_error_on_jwks_fetch_returns_retryable
#[tokio::test]
async fn network_error_on_jwks_fetch_returns_retryable() {
    // Port 1 is reserved (TCPMUX) and never listens for HTTP.
    let cache = JwksCache::new(Url::parse("http://127.0.0.1:1/jwks.json").unwrap()).unwrap();
    let verifier = eddsa_verifier(Arc::new(cache));
    let key = random_key("k");
    let now = chrono::Utc::now().timestamp();
    let token = mint_jwt(
        &key.private_jwk,
        "k",
        json!({ "sub": "u", "exp": now + 60 }),
    );
    let err = verifier.verify(&token).await.unwrap_err();
    // Network error on JWKS fetch → `JwksFetch` or `Transport`.
    assert!(matches!(
        err,
        OAuth2Error::JwksFetch { .. } | OAuth2Error::Transport(_)
    ));
}
