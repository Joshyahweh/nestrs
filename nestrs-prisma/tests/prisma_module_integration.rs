use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use nestrs_prisma::{
    deployment::{github_actions_deploy_workflow, DatabaseKind, PackageManager, PgfenceOptions},
    prisma_db_push_command, prisma_generate_command, prisma_migrate_deploy_command, PrismaModule,
    PrismaOptions, PrismaService, DEFAULT_SCHEMA_PATH,
};
use tower::util::ServiceExt;

pub struct AppService {
    prisma: Arc<PrismaService>,
}

impl AppService {
    fn new(prisma: Arc<PrismaService>) -> Self {
        Self { prisma }
    }
}

impl Injectable for AppService {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::new(registry.get::<PrismaService>()))
    }
}

#[controller(prefix = "/db", version = "v1")]
pub struct AppController;

impl AppController {
    #[get("/health")]
    pub async fn health(State(svc): State<Arc<AppService>>) -> String {
        svc.prisma.health().to_string()
    }
}

impl_routes!(AppController, state AppService => [
    GET "/health" with () => AppController::health,
]);

#[module(
    imports = [PrismaModule],
    controllers = [AppController],
    providers = [AppService],
)]
pub struct AppModule;

#[tokio::test]
async fn prisma_module_exports_service_to_importing_module() {
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("file:./integration.db")
            .pool_min(1)
            .pool_max(8)
            .schema_path(DEFAULT_SCHEMA_PATH),
    );

    let router = NestFactory::create::<AppModule>().into_router();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/db/health")
                .method("GET")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("router should serve request");

    assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn prisma_generation_command_uses_schema_path() {
    let cmd = prisma_generate_command("prisma/schema.prisma");
    assert_eq!(cmd, "cargo prisma generate --schema prisma/schema.prisma");
}

#[test]
fn prisma_deploy_commands_are_exposed() {
    assert_eq!(prisma_migrate_deploy_command(), "npx prisma migrate deploy");
    assert_eq!(prisma_db_push_command(), "npx prisma db push");
    assert_eq!(
        PrismaModule::deploy_command_hint(),
        "npx prisma migrate deploy"
    );
}

#[test]
fn github_actions_workflow_generation_covers_relational_and_mongo() {
    let relational = github_actions_deploy_workflow(
        PackageManager::Npm,
        DatabaseKind::PostgreSql,
        PgfenceOptions {
            enabled: true,
            max_risk: "medium",
        },
    );
    assert!(relational.contains("prisma/migrations/**"));
    assert!(relational.contains("npx prisma migrate deploy"));
    assert!(relational.contains("@flvmnt/pgfence analyze"));

    let mongo = github_actions_deploy_workflow(
        PackageManager::Npm,
        DatabaseKind::MongoDb,
        PgfenceOptions {
            enabled: true,
            max_risk: "medium",
        },
    );
    assert!(mongo.contains("npx prisma db push"));
    assert!(!mongo.contains("@flvmnt/pgfence analyze"));
}
