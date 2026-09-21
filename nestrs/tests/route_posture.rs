#![cfg(feature = "test-hooks")]

//! Route posture: `#[public]` / `#[use_guards]` metadata + `require_route_posture`.
//!
//! These tests share process-global registries — run serially.

use axum::http::request::Parts;
use nestrs::prelude::*;
use nestrs::{assert_route_posture, POSTURE_METADATA_KEY};
use nestrs_core::{CanActivate, GuardError, MetadataRegistry, RouteRegistry};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct AllowGuard;

#[async_trait]
impl CanActivate for AllowGuard {
    async fn can_activate(&self, _parts: &Parts) -> Result<(), GuardError> {
        Ok(())
    }
}

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct AppController;

#[routes(state = AppState)]
impl AppController {
    #[get("/public")]
    #[public]
    async fn open() -> &'static str {
        "ok"
    }

    #[get("/secret")]
    #[use_guards(AllowGuard)]
    async fn secret() -> &'static str {
        "secret"
    }

    #[get("/forgotten")]
    async fn forgotten() -> &'static str {
        "oops"
    }
}

#[module(controllers = [AppController], providers = [AppState])]
struct AppModule;

fn reset() {
    RouteRegistry::clear_for_tests();
    MetadataRegistry::clear_for_tests();
}

#[tokio::test]
async fn posture_end_to_end() {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset();
    let app = NestFactory::create::<AppModule>();

    let public_handler = RouteRegistry::list()
        .into_iter()
        .find(|r| r.path.ends_with("/public"))
        .expect("public route")
        .handler
        .to_string();
    let secret_handler = RouteRegistry::list()
        .into_iter()
        .find(|r| r.path.ends_with("/secret"))
        .expect("secret route")
        .handler
        .to_string();
    let forgotten_handler = RouteRegistry::list()
        .into_iter()
        .find(|r| r.path.ends_with("/forgotten"))
        .expect("forgotten route")
        .handler
        .to_string();

    assert_eq!(
        MetadataRegistry::get(&public_handler, POSTURE_METADATA_KEY).as_deref(),
        Some("public")
    );
    assert_eq!(
        MetadataRegistry::get(&secret_handler, POSTURE_METADATA_KEY).as_deref(),
        Some("guarded")
    );
    assert!(
        MetadataRegistry::get(&forgotten_handler, POSTURE_METADATA_KEY).is_none(),
        "forgotten must lack posture, got {:?}",
        MetadataRegistry::get(&forgotten_handler, POSTURE_METADATA_KEY)
    );

    let err = app.assert_route_posture().expect_err("forgotten");
    assert!(err.unguarded.iter().any(|r| r.path.ends_with("/forgotten")));

    // Mark forgotten public → posture ok.
    MetadataRegistry::set(&forgotten_handler, POSTURE_METADATA_KEY, "public");
    assert!(assert_route_posture(&[]).is_ok());

    // require_route_posture panics when forgotten again.
    MetadataRegistry::set(&forgotten_handler, POSTURE_METADATA_KEY, "");
    // empty string is not public/guarded
    let app2 = NestFactory::create::<AppModule>().require_route_posture();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = app2.into_router();
    }));
    assert!(result.is_err(), "expected posture panic");
}
