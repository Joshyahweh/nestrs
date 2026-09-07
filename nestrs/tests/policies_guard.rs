#![cfg(all(feature = "authz", feature = "authn"))]

//! End-to-end tests for `PoliciesGuard` chained with `AuthnGuard`:
//!  * Returns 200 when the resolved Ability grants the route's `#[check_policies(...)]`
//!  * Returns 403 when the Ability denies
//!  * Chains correctly with `AuthnGuard` (principal → ability → check_policies)

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::middleware;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use nestrs::prelude::*;
use nestrs::{
    install_authn_middleware, install_policies_middleware, Ability, Action, AuthnModule,
    AuthnOptions, Conditions, JwtService, PoliciesModule, PoliciesOptions,
};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tower::util::ServiceExt;

// -- Shared Ed25519 keypair ----------------------------------------------------

struct TestKeys {
    public_pem: String,
    encoding: EncodingKey,
}

fn test_keys() -> &'static TestKeys {
    static KEYS: OnceLock<TestKeys> = OnceLock::new();
    KEYS.get_or_init(|| {
        use pkcs8::{EncodePrivateKey, LineEnding};
        let mut csprng = rand::rngs::OsRng;
        let kp = ed25519_dalek::SigningKey::generate(&mut csprng);
        let private_pem = kp.to_pkcs8_pem(LineEnding::LF).expect("encode private pem");
        let public_pem = build_ed25519_public_pem(kp.verifying_key().to_bytes());
        let encoding = EncodingKey::from_ed_pem(private_pem.as_bytes()).expect("encoding key");
        TestKeys {
            public_pem,
            encoding,
        }
    })
}

fn build_ed25519_public_pem(raw_pub: [u8; 32]) -> String {
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

#[allow(clippy::manual_div_ceil)] // pre-existing test helper
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

// -- App with both Authn and Policies ------------------------------------------

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/p")]
struct PController;

#[routes(state = AppState)]
impl PController {
    #[get("/read-post")]
    #[check_policies("read:Post")]
    #[use_guards(AuthnGuard, PoliciesGuard)]
    async fn read_post() -> &'static str {
        "read-ok"
    }

    #[get("/admin")]
    #[check_policies("manage:Org")]
    #[use_guards(AuthnGuard, PoliciesGuard)]
    async fn admin() -> &'static str {
        "admin-ok"
    }
}

#[module(
    imports = [
        AuthnModule::register(test_options()),
        PoliciesModule::register(PoliciesOptions::new(
            Ability::builder()
                .can(Action::Read, "Post")
                .can(Action::Manage, "Org")
                .build()
        )),
    ],
    providers = [AppState],
    controllers = [PController],
)]
struct AppModule;

fn test_router() -> axum::Router {
    let jwt_svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let ability: Arc<Ability> = Arc::new(
        Ability::builder()
            .can(Action::Read, "Post")
            .can(Action::Manage, "Org")
            .build(),
    );
    let app = NestFactory::create::<AppModule>();
    let router: axum::Router = app.into_router();
    router
        .layer(middleware::from_fn_with_state(
            jwt_svc,
            install_authn_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            ability,
            install_policies_middleware,
        ))
}

#[tokio::test]
async fn policies_guard_returns_401_when_no_token() {
    let router = test_router();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/read-post")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn policies_guard_returns_200_when_action_granted() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/read-post")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "read-ok");
}

#[tokio::test]
async fn policies_guard_returns_200_when_manage_action_granted() {
    let router = test_router();
    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/admin")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn policies_guard_returns_403_when_action_not_granted() {
    // A new ability that grants NOTHING; the same route should now 403.
    let jwt_svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let empty_ability: Arc<Ability> = Arc::new(Ability::builder().build());
    let app = NestFactory::create::<AppModule>();
    let router: axum::Router = app.into_router();
    let router = router
        .layer(middleware::from_fn_with_state(
            jwt_svc,
            install_authn_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            empty_ability,
            install_policies_middleware,
        ));

    let token = mint_token(json!({
        "sub": "user-1",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/read-post")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn policies_guard_chains_after_authn_guard() {
    // Verify the chain: AuthnGuard runs first (it has to populate the
    // PrincipalIdentity that PoliciesGuard consults). No token => 401 (authn),
    // not 403 (policies), so the chain ran in the expected order.
    let router = test_router();
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn policies_guard_returns_403_when_principal_lacks_conditions() {
    // Tenant-scoped read: only "tenant_id": 7 can read Posts.
    let mut conds = Conditions::new();
    conds.insert("tenant_id".into(), json!(7));
    let ability = Ability::builder()
        .can_with_conditions(Action::Read, "Post", conds)
        .can(Action::Manage, "Org")
        .build();
    let jwt_svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let ability_arc: Arc<Ability> = Arc::new(ability);
    let app = NestFactory::create::<AppModule>();
    let router: axum::Router = app.into_router();
    let router = router
        .layer(middleware::from_fn_with_state(
            jwt_svc,
            install_authn_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            ability_arc,
            install_policies_middleware,
        ));

    // The principal instance has no "tenant_id" attribute that matches 7,
    // so the read should be denied.
    let token = mint_token(json!({
        "sub": "user-tenant-9",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let res = router
        .oneshot(
            Request::builder()
                .uri("/p/read-post")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("serve");
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
