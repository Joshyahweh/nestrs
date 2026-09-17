use nestrs::prelude::*;

#[dto]
pub struct PingDto {
    #[IsString]
    pub message: String,
}

#[controller(prefix = "/")]
pub struct AppController;

impl AppController {
    #[get("/")]
    pub async fn root() -> &'static str {
        "Hello from nestrs"
    }
}

#[derive(Default)]
#[injectable]
pub struct AppService;

impl_routes!(AppController, state AppService => [
    GET "/" with () => AppController::root,
]);

#[module(
    controllers = [AppController],
    providers = [AppService],
)]
pub struct AppModule;

#[tokio::main]
async fn main() {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(3000);

    NestFactory::create::<AppModule>()
        .set_global_prefix("api")
        .use_request_id()
        .use_request_tracing(RequestTracingOptions::builder().skip_paths(["/metrics"]))
        .enable_metrics("/metrics")
        .enable_health_check("/health")
        // OpenAPI + Swagger UI (add `features = ["openapi"]` on `nestrs` in Cargo.toml):
        // .enable_openapi()
        .enable_production_errors_from_env()
        .listen_graceful(port)
        .await;
}
