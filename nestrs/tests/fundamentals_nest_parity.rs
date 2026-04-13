//! Coverage for NestJS fundamentals parity: custom providers, ModuleRef, Discovery, lazy imports,
//! circular provider detection.

use axum::body::Body;
use axum::http::Request;
use nestrs::core::{
    DiscoveryService, DynamicModule, HostType, ModuleRef, ProviderRegistry, ProviderScope,
};
use nestrs::prelude::*;
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Default)]
#[injectable]
struct LazyLeafState;

#[controller(prefix = "/lazy-leaf")]
struct LazyLeafController;

impl LazyLeafController {
    #[get("/")]
    async fn ok() -> &'static str {
        "leaf"
    }
}

impl_routes!(LazyLeafController, state LazyLeafState => [
    GET "/" with () => LazyLeafController::ok,
]);

#[module(
    imports = [],
    controllers = [LazyLeafController],
    providers = [LazyLeafState],
)]
struct LazyLeafModule;

#[module(
    imports = [lazy_module::<LazyLeafModule>()],
    controllers = [],
    providers = [],
)]
struct LazyParentModule;

#[tokio::test]
async fn lazy_module_import_builds_routes_once() {
    let app = NestFactory::create::<LazyParentModule>()
        .use_execution_context()
        .into_router();

    let res = app
        .oneshot(
            Request::builder()
                .uri("/lazy-leaf")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
}

#[test]
fn custom_providers_use_value_and_factory() {
    let mut r = ProviderRegistry::new();
    r.register_use_value(Arc::new(41_i32));
    assert_eq!(*r.get::<i32>(), 41);

    r.register_use_factory::<String, _>(ProviderScope::Singleton, |reg| {
        Arc::new(format!("x={}", reg.get::<i32>()))
    });
    assert_eq!(r.get::<String>().as_str(), "x=41");
}

#[test]
fn module_ref_and_discovery() {
    let mut r = ProviderRegistry::new();
    r.register_use_value(Arc::new(7_u64));
    let r = Arc::new(r);
    let mref = ModuleRef::new(Arc::clone(&r));
    assert_eq!(*mref.get::<u64>(), 7);

    let disc = DiscoveryService::new(mref);
    assert!(!disc.get_providers().is_empty());
}

#[test]
fn dynamic_module_lazy_matches_macro_semantics() {
    let a = DynamicModule::lazy::<LazyLeafModule>();
    let b = DynamicModule::lazy::<LazyLeafModule>();
    assert_eq!(a.exports, b.exports);
}

struct CircA;
struct CircB;

#[async_trait]
impl Injectable for CircA {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        let _ = registry.get::<CircB>();
        Arc::new(Self)
    }
}

#[async_trait]
impl Injectable for CircB {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        let _ = registry.get::<CircA>();
        Arc::new(Self)
    }
}

#[test]
#[should_panic(expected = "Circular provider dependency detected")]
fn circular_provider_construct_panics_with_chain_message() {
    let mut r = ProviderRegistry::new();
    r.register::<CircA>();
    r.register::<CircB>();
    let _ = r.get::<CircA>();
}

#[test]
fn execution_context_from_parts() {
    use axum::http::Method;
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/items?q=1")
        .body(Body::empty())
        .unwrap();
    let (parts, _) = req.into_parts();
    let ctx = nestrs::ExecutionContext::from_http_parts(&parts);
    assert_eq!(ctx.get_type(), HostType::Http);
    assert_eq!(ctx.method, Method::GET);
    assert!(ctx.path_and_query.contains("/api/items"));
}
