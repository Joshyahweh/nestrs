#![cfg(feature = "authn")]

//! Authn module integration tests:
//!  * JWT (EdDSA): valid/expired/wrong-alg/missing/role mismatch
//!  * Argon2id: PHC string shape, correct/wrong password, malformed encoded

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use nestrs::prelude::*;
use nestrs::{AuthnModule, AuthnOptions, JwtService, PasswordHasher};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tower::util::ServiceExt;

// -- Shared Ed25519 keypair for the test binary ----------------------------

struct TestKeys {
    public_pem: String,
    encoding: EncodingKey,
    #[allow(dead_code)] // kept for parity with `encoding`; tests rely on `encoding` directly.
    decoding_pem: String,
}

fn test_keys() -> &'static TestKeys {
    static KEYS: OnceLock<TestKeys> = OnceLock::new();
    KEYS.get_or_init(|| {
        use pkcs8::{EncodePrivateKey, LineEnding};
        let mut csprng = rand::rngs::OsRng;
        let kp = ed25519_dalek::SigningKey::generate(&mut csprng);
        let private_pem = kp.to_pkcs8_pem(LineEnding::LF).expect("encode private pem");
        // Build the SPKI public PEM manually from the raw 32-byte public key.
        let public_pem = build_ed25519_public_pem(kp.verifying_key().to_bytes());
        let encoding = EncodingKey::from_ed_pem(private_pem.as_bytes()).expect("encoding key");
        let _ = jsonwebtoken::DecodingKey::from_ed_pem(public_pem.as_bytes())
            .expect("decoding key sanity");
        TestKeys {
            public_pem: public_pem.clone(),
            encoding,
            decoding_pem: public_pem,
        }
    })
}

fn build_ed25519_public_pem(raw_pub: [u8; 32]) -> String {
    // Ed25519 SubjectPublicKeyInfo (SPKI) DER prefix for the 32-byte raw key.
    const PREFIX: [u8; 12] = [
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    let mut spki = Vec::with_capacity(PREFIX.len() + raw_pub.len());
    spki.extend_from_slice(&PREFIX);
    spki.extend_from_slice(&raw_pub);
    let b64 = base64_encode(&spki);
    let mut out = String::from("-----BEGIN PUBLIC KEY-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).expect("ascii"));
        out.push('\n');
    }
    out.push_str("-----END PUBLIC KEY-----\n");
    out
}

#[allow(clippy::manual_div_ceil)] // pre-existing test helper, kept for parity
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn test_options() -> AuthnOptions {
    AuthnOptions::new(test_keys().public_pem.clone()).with_leeway_seconds(60)
}

fn mint_token(claims: serde_json::Value) -> String {
    let header = Header::new(Algorithm::EdDSA);
    encode(&header, &claims, &test_keys().encoding).expect("encode")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// -- Test app ---------------------------------------------------------------

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/auth")]
struct AuthController;

#[routes(state = AppState)]
impl AuthController {
    #[get("/me")]
    #[use_guards(AuthnGuard)]
    async fn me(principal: Principal) -> String {
        principal.0.subject
    }

    #[get("/me-optional")]
    async fn me_opt(principal: OptionalPrincipal) -> String {
        match principal.0 {
            Some(p) => format!("authed:{}", p.subject),
            None => "anonymous".to_string(),
        }
    }

    #[get("/admin")]
    #[roles("admin")]
    #[use_guards(AuthnGuard)]
    async fn admin() -> &'static str {
        "admin-ok"
    }
}

#[module(
    imports = [AuthnModule::register(test_options())],
    providers = [AppState],
    controllers = [AuthController]
)]
struct AppModule;

// -- AuthnModule service unit tests ----------------------------------------

#[tokio::test]
async fn jwt_service_verify_returns_principal_for_valid_token() {
    let svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["admin", "user"],
        "exp": now() + 600,
    }));
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let parts = req.into_parts().0;
    let principal = svc.verify(&parts).expect("verify");
    assert_eq!(principal.subject, "user-1");
    assert_eq!(principal.roles, vec!["admin", "user"]);
}

#[tokio::test]
async fn jwt_service_verify_rejects_expired_token() {
    let svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let token = mint_token(json!({
        "sub": "user-1",
        "exp": now() - 600,
    }));
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let parts = req.into_parts().0;
    assert!(svc.verify(&parts).is_err(), "expired token must fail");
}

#[tokio::test]
async fn jwt_service_verify_rejects_wrong_algorithm() {
    // Sign an HS256 token using the public-key PEM bytes as the symmetric secret.
    // jsonwebtoken must reject it because the validator pins EdDSA.
    use jsonwebtoken::{encode, EncodingKey, Header};
    let pem_bytes = test_keys().public_pem.as_bytes();
    let hs = EncodingKey::from_secret(pem_bytes);
    let bad = encode(
        &Header::new(Algorithm::HS256),
        &json!({ "sub": "x", "exp": now() + 600 }),
        &hs,
    )
    .expect("encode");
    let svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let req = Request::builder()
        .header(header::AUTHORIZATION, format!("Bearer {bad}"))
        .body(Body::empty())
        .unwrap();
    let parts = req.into_parts().0;
    assert!(
        svc.verify(&parts).is_err(),
        "HS256 against EdDSA key must fail"
    );
}

#[tokio::test]
async fn jwt_service_verify_rejects_missing_authorization_header() {
    let svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let req = Request::builder().body(Body::empty()).unwrap();
    let parts = req.into_parts().0;
    assert!(svc.verify(&parts).is_err());
}

// -- Argon2id ---------------------------------------------------------------

#[tokio::test]
async fn argon2id_hash_produces_phc_string_with_argon2id_prefix() {
    let h = nestrs::Argon2idPasswordHasher::new(nestrs::Argon2idParams::default());
    let encoded = <nestrs::Argon2idPasswordHasher as PasswordHasher>::hash(&h, "hunter2")
        .await
        .expect("hash");
    assert!(
        encoded.starts_with("$argon2id$"),
        "expected PHC argon2id prefix, got {encoded}"
    );
}

#[tokio::test]
async fn argon2id_verify_returns_true_for_correct_false_for_wrong() {
    let h = nestrs::Argon2idPasswordHasher::new(nestrs::Argon2idParams::default());
    let encoded = <nestrs::Argon2idPasswordHasher as PasswordHasher>::hash(&h, "hunter2")
        .await
        .expect("hash");
    let ok = <nestrs::Argon2idPasswordHasher as PasswordHasher>::verify(&h, "hunter2", &encoded)
        .await
        .expect("verify ok");
    assert!(ok, "matching password must verify");
    let bad = <nestrs::Argon2idPasswordHasher as PasswordHasher>::verify(&h, "hunter3", &encoded)
        .await
        .expect("verify bad");
    assert!(!bad, "wrong password must not verify");
}

#[tokio::test]
async fn argon2id_verify_returns_err_for_malformed_encoded_hash() {
    let h = nestrs::Argon2idPasswordHasher::new(nestrs::Argon2idParams::default());
    let r = <nestrs::Argon2idPasswordHasher as PasswordHasher>::verify(&h, "x", "not-a-phc-string")
        .await;
    assert!(r.is_err(), "malformed encoded must error");
}

// -- AuthnGuard + Principal extractor via the install middleware -----------

fn test_router() -> axum::Router {
    use axum::middleware;
    use nestrs::install_authn_middleware;
    let jwt_svc = nestrs::build_jwt_service(&test_options());
    let app = NestFactory::create::<AppModule>();
    let router: axum::Router = app.into_router();
    router.layer(middleware::from_fn_with_state(
        jwt_svc,
        install_authn_middleware,
    ))
}

#[tokio::test]
async fn authn_guard_returns_401_when_no_authorization_header() {
    let router = test_router();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authn_guard_returns_200_and_principal_for_valid_token() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "user-1");
}

#[tokio::test]
async fn authn_guard_returns_401_for_expired_token() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "exp": now() - 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/me")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authn_guard_returns_403_when_role_metadata_unmet() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/admin")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn authn_guard_returns_200_when_role_metadata_met() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["admin", "user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/admin")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn optional_principal_yields_none_without_token() {
    let router = test_router();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/me-optional")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "anonymous");
}

#[tokio::test]
async fn optional_principal_yields_some_with_valid_token() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-7",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/auth/me-optional")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "authed:user-7");
}
