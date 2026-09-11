#![cfg(all(feature = "authn", feature = "authz-row-level"))]

//! Full HTTP-stack survival test for row-level authorization:
//! minted EdDSA JWT → `install_authn_middleware` (installs the ambient
//! `Principal` task-local) → `install_policies_middleware` (installs the
//! request `Ability`) → handler's `CrudService` auto-applies the row
//! predicate. No test scaffolding touches the task-locals — this is the
//! same plumbing a real request goes through.

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::middleware;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use nestrs::policies::Principal;
use nestrs::predicates::AuthorIsCurrentUser;
use nestrs::prelude::*;
use nestrs::{
    current_principal, install_authn_middleware, install_policies_middleware, Ability, Action,
    AuthnModule, AuthnOptions, CrudService, Entity, JwtService, NestFactory, PoliciesModule,
    PoliciesOptions, Repository,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, OnceLock};
use tower::util::ServiceExt;

// -- Shared Ed25519 keypair (same helpers as policies_guard.rs) -----------------

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
        let encoding = EncodingKey::from_ed_pem(private_pem.as_bytes()).expect("encoding key");
        TestKeys {
            public_pem: build_ed25519_public_pem(kp.verifying_key().to_bytes()),
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

fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
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

// -- Entity + pool ----------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Post {
    id: Option<i64>,
    author: String,
    body: String,
}

impl Entity for Post {
    const TABLE: &'static str = "posts";
    fn id(&self) -> Option<i64> {
        self.id
    }
    fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        let id: i64 = row.try_get("id")?;
        let data: String = row.try_get("data")?;
        let parsed: serde_json::Value =
            serde_json::from_str(&data).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let s = |k: &str| {
            parsed
                .get(k)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        Ok(Post {
            id: Some(id),
            author: s("author"),
            body: s("body"),
        })
    }
}

static POOL: OnceLock<Arc<sqlx::AnyPool>> = OnceLock::new();

fn pool() -> Arc<sqlx::AnyPool> {
    POOL.get()
        .cloned()
        .expect("pool initialized by setup_router before the router is used")
}

// -- HTTP handlers: each builds a CrudService from the shared pool ----------------

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/p")]
struct PController;

#[routes(state = AppState)]
impl PController {
    /// Create a post authored as the CURRENT principal — must succeed.
    #[get("/create-own")]
    async fn create_own() -> String {
        let svc = CrudService::<Post>::new(pool());
        let p: Arc<Principal> = current_principal().expect("principal from authn middleware");
        match svc
            .create(json!({ "author": p.subject, "body": "hi" }))
            .await
        {
            Ok(created) => format!("created:{}", created.id.unwrap()),
            Err(e) => format!("err:{e}"),
        }
    }

    /// Create a post authored as SOMEONE ELSE — the predicate must deny it.
    #[get("/create-spoofed")]
    async fn create_spoofed() -> String {
        let svc = CrudService::<Post>::new(pool());
        match svc
            .create(json!({ "author": "mallory", "body": "spoof" }))
            .await
        {
            Ok(_) => "SPOOF LEAKED".to_string(),
            Err(e) => format!("denied:{e}"),
        }
    }

    /// List posts — must contain only rows visible to the principal.
    #[get("/posts")]
    async fn list_posts() -> String {
        let svc = CrudService::<Post>::new(pool());
        match svc.list().await {
            Ok(rows) => {
                let mut authors: Vec<&str> = rows.iter().map(|r| r.author.as_str()).collect();
                authors.sort();
                authors.join(",")
            }
            Err(e) => format!("err:{e}"),
        }
    }
}

#[module(
    imports = [
        AuthnModule::register(test_options()),
        PoliciesModule::register(PoliciesOptions::new(
            Ability::builder()
                .can_with_predicate(Action::Create, "posts", vec![], AuthorIsCurrentUser::new())
                .can_with_predicate(Action::Read, "posts", vec![], AuthorIsCurrentUser::new())
                .build()
        )),
    ],
    providers = [AppState],
    controllers = [PController],
)]
struct AppModule;

async fn setup_router() -> axum::Router {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-row-level-http-{}.sqlite",
        std::process::id()
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let p = Arc::new(
        sqlx::any::AnyPoolOptions::new()
            .max_connections(4)
            .connect(&url)
            .await
            .expect("connect sqlite"),
    );
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS posts \
         (id INTEGER PRIMARY KEY AUTOINCREMENT, data TEXT NOT NULL)",
    )
    .execute(p.as_ref())
    .await
    .expect("create posts");
    POOL.set(p).expect("pool set once");

    // Seed one row authored by bob through the repository primitive (no
    // ability needed — it bypasses row-level authz by design).
    let repo = Repository::<Post>::new(pool());
    repo.repo_crud_create(&Post {
        id: None,
        author: "bob".into(),
        body: "bob's secret".into(),
    })
    .await
    .expect("seed");

    let jwt_svc: Arc<JwtService> = nestrs::build_jwt_service(&test_options());
    let ability: Arc<Ability> = Arc::new(
        Ability::builder()
            .can_with_predicate(Action::Create, "posts", vec![], AuthorIsCurrentUser::new())
            .can_with_predicate(Action::Read, "posts", vec![], AuthorIsCurrentUser::new())
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

async fn get(router: axum::Router, uri: &str, token: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = router
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("serve");
    let status = res.status();
    let body = to_bytes(res.into_body(), 64 * 1024).await.expect("body");
    (status, String::from_utf8_lossy(&body).to_string())
}

#[tokio::test]
async fn crud_service_sees_only_own_rows_through_the_http_stack() {
    let router = setup_router().await;
    let token = mint_token(json!({
        "sub": "alice",
        "roles": ["user"],
        "exp": now() + 600,
    }));

    // No token => the authn middleware installs no principal; the
    // deny-closed CrudService refuses to even list (fail-loud, no rows).
    let (status, body) = get(router.clone(), "/p/posts", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("requires a Principal"), "{body}");

    // With alice's token: create own => ok.
    let (status, body) = get(router.clone(), "/p/create-own", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.starts_with("created:"), "{body}");

    // Spoofed author => denied by the predicate.
    let (status, body) = get(router.clone(), "/p/create-spoofed", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.starts_with("denied:") && body.contains("policy denied"),
        "{body}"
    );

    // List => only alice's rows (bob's seeded row is invisible).
    let (status, body) = get(router.clone(), "/p/posts", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "alice");

    // Bob's token: his own view — his seeded row, never alice's.
    let bob_token = mint_token(json!({
        "sub": "bob",
        "roles": ["user"],
        "exp": now() + 600,
    }));
    let (status, body) = get(router.clone(), "/p/posts", Some(&bob_token)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, "bob");
}
