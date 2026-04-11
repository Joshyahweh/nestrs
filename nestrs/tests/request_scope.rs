use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::util::ServiceExt;

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Default)]
#[injectable]
struct AppState;

struct ReqService {
    id: usize,
}

impl Injectable for ReqService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
        })
    }

    fn scope() -> ProviderScope {
        ProviderScope::Request
    }
}

#[controller(prefix = "/", version = "v1")]
struct ReqController;

#[routes(state = AppState)]
impl ReqController {
    #[get("/")]
    async fn root(
        RequestScoped(a): RequestScoped<ReqService>,
        RequestScoped(b): RequestScoped<ReqService>,
    ) -> String {
        assert!(
            Arc::ptr_eq(&a, &b),
            "request-scoped provider should be cached for the duration of one request"
        );
        a.id.to_string()
    }
}

#[module(controllers = [ReqController], providers = [AppState, ReqService])]
struct AppModule;

#[tokio::test]
async fn request_scope_caches_per_request_and_resets_between_requests() {
    let router = NestFactory::create::<AppModule>()
        .use_request_scope()
        .into_router();

    let r1 = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(r1.status(), StatusCode::OK);
    let b1 = to_bytes(r1.into_body(), 1024).await.expect("body");
    let id1 = String::from_utf8(b1.to_vec()).expect("utf8");

    let r2 = router
        .oneshot(
            Request::builder()
                .uri("/v1")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    assert_eq!(r2.status(), StatusCode::OK);
    let b2 = to_bytes(r2.into_body(), 1024).await.expect("body");
    let id2 = String::from_utf8(b2.to_vec()).expect("utf8");

    assert_ne!(
        id1, id2,
        "request-scoped providers should not be reused across requests"
    );
}
