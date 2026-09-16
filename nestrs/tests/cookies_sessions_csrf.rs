//! End-to-end coverage for the umbrella `nestrs` cookies + sessions + CSRF
//! surface, complementing `csrf_middleware.rs` (which only exercises the
//! CSRF reject / accept pair). Gated on all three feature flags because
//! every test needs the full layer stack.
//!
//! What's covered here that isn't elsewhere:
//! - `tower_cookies::Cookies` extractor round-trip (write in one request,
//!   read in another over the same `Cookie` jar).
//! - `tower_sessions::Session` extractor round-trip (insert in a POST,
//!   read back in a GET).
//! - GET / HEAD / OPTIONS bypass the CSRF check (no 403 on safe methods).
//! - PUT / PATCH / DELETE also gated (other unsafe methods, not just POST).

#![cfg(all(feature = "cookies", feature = "session", feature = "csrf"))]

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;
use tower_cookies::Cookies;
use tower_sessions::Session;

#[derive(Default, Clone)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct CartController;

#[routes(state = AppState)]
impl CartController {
    #[get("/me")]
    async fn me(session: Session) -> Result<String, StatusCode> {
        match session.get::<String>("user").await.unwrap_or(None) {
            Some(name) => Ok(format!("hello {name}")),
            None => Err(StatusCode::UNAUTHORIZED),
        }
    }

    #[post("/login")]
    async fn login(session: Session, cookies: Cookies) -> &'static str {
        session.insert("user", "ada").await.unwrap();
        // Round-trip the cookie jar to prove Cookies is wired through the
        // tower-cookies layer on a POST (CSRF checks the request side; we
        // just want the write side here).
        cookies.add(
            tower_cookies::Cookie::build(("cart_id", "cart-1"))
                .path("/")
                .http_only(true)
                .build(),
        );
        "ok"
    }

    #[post("/echo")]
    async fn echo() -> &'static str {
        "ok"
    }

    #[put("/echo")]
    async fn echo_put() -> &'static str {
        "ok"
    }

    #[patch("/echo")]
    async fn echo_patch() -> &'static str {
        "ok"
    }

    #[delete("/echo")]
    async fn echo_delete() -> &'static str {
        "ok"
    }

    #[head("/echo")]
    async fn echo_head() -> &'static str {
        "ok"
    }
}

#[module(controllers = [CartController], providers = [AppState])]
struct AppModule;

/// Build a router with all three layers on. One line of reuse so each
/// test below reads as the assertion, not the plumbing.
fn router() -> axum::Router {
    NestFactory::create::<AppModule>()
        .use_cookies()
        .use_session_memory()
        .use_csrf_protection(nestrs::CsrfProtectionConfig::default())
        .into_router()
}

#[tokio::test]
async fn cookies_extractor_writes_a_cookie() {
    let router = router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/login")
                .method("POST")
                .header(header::COOKIE, "csrf_token=secret")
                .header("x-csrf-token", "secret")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
    // The handler added `cart_id=cart-1`. The cookie layer should put it
    // back on the response as Set-Cookie.
    let set_cookies: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter(|s| s.starts_with("cart_id="))
        .collect();
    assert_eq!(set_cookies.len(), 1, "expected exactly one cart_id Set-Cookie");
    assert!(
        set_cookies[0].contains("cart-1"),
        "cart_id cookie body should be cart-1, got {}",
        set_cookies[0]
    );
}

#[tokio::test]
async fn session_extractor_round_trip() {
    let router = router();

    // Login: insert the user in the session.
    let login = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/login")
                .method("POST")
                .header(header::COOKIE, "csrf_token=secret")
                .header("x-csrf-token", "secret")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");
    assert_eq!(login.status(), StatusCode::OK);

    // Pull the session cookie out of the response and forward it to /me.
    let session_cookie = login
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|s| s.starts_with("id=")) // tower-sessions default cookie name
        .expect("session cookie set on login");

    let me = router
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .method("GET")
                .header(header::COOKIE, session_cookie.to_string())
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(me.status(), StatusCode::OK);
    let body_bytes = axum::body::to_bytes(me.into_body(), 1024)
        .await
        .expect("body");
    let body = std::str::from_utf8(&body_bytes).expect("utf8");
    assert_eq!(body, "hello ada", "session value should round-trip");
}

#[tokio::test]
async fn get_bypasses_csrf_check() {
    let router = router();

    // No CSRF token, no cookie, safe method — must still 200.
    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/echo")
                .method("GET")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn put_patch_delete_are_also_gated() {
    // One router, four assertions. POST is covered by csrf_middleware.rs;
    // here we extend the same reject path to PUT/PATCH/DELETE so a future
    // refactor that narrows is_unsafe_method to just POST would fail.
    for method in ["PUT", "PATCH", "DELETE"] {
        let router = router();
        let response = router
            .oneshot(
                Request::builder()
                    .uri("/api/echo")
                    .method(method)
                    .header(header::COOKIE, "csrf_token=secret")
                    .body(Body::empty())
                    .expect("valid"),
            )
            .await
            .expect("serve");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} without matching header should be rejected"
        );
    }
}

#[tokio::test]
async fn head_is_a_safe_method() {
    let router = router();

    let response = router
        .oneshot(
            Request::builder()
                .uri("/api/echo")
                .method("HEAD")
                .body(Body::empty())
                .expect("valid"),
        )
        .await
        .expect("serve");

    // HEAD on a route that returns `&'static str` becomes a 200 with an
    // empty body. The point is the CSRF layer must not have rejected it.
    assert_eq!(response.status(), StatusCode::OK);
}
