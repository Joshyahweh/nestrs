use axum::body::to_bytes;
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

struct CatsService {
    label: &'static str,
}

#[nestrs::async_trait]
impl nestrs::core::Injectable for CatsService {
    fn construct(_registry: &nestrs::core::ProviderRegistry) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self { label: "real" })
    }
}

#[controller(prefix = "/cats")]
struct CatsController;

#[routes(state = AppState)]
impl CatsController {
    #[get("/")]
    async fn list(RequestScoped(svc): RequestScoped<CatsService>) -> String {
        svc.label.to_string()
    }
}

#[module(controllers = [CatsController], providers = [AppState, CatsService])]
struct AppModule;

#[tokio::test]
async fn testing_module_overrides_provider_instance_and_serves_router() {
    let app = TestingModule::builder::<AppModule>()
        .configure_http(|app| app.use_request_scope())
        .override_provider(std::sync::Arc::new(CatsService { label: "mock" }))
        .compile()
        .await;

    let svc = app.get::<CatsService>();
    assert_eq!(svc.label, "mock");

    let client = app.http_client();
    let res = client.get("/cats").send().await;
    assert_eq!(res.status(), 200);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), "mock");
}
