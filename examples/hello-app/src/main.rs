use nestrs::prelude::*;
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub name: String,
}

#[injectable]
pub struct AppService {
    prisma: Arc<PrismaService>,
}

impl AppService {
    pub fn get_hello(&self) -> &'static str {
        "Hello World"
    }

    /// Persists the user and returns the stored row. The prisma facade exposes raw SQL
    /// (`execute`), so string literals are escaped manually here; prefer bound parameters
    /// via `sqlx` for anything beyond a demo.
    pub async fn create_user(&self, dto: CreateUserDto) -> Result<UserResponse, String> {
        let email = dto.email.replace('\'', "''");
        let name = dto.name.replace('\'', "''");
        self.prisma
            .execute(&format!(
                r#"INSERT INTO "User" ("email", "name") VALUES ('{email}', '{name}')"#
            ))
            .await?;
        Ok(UserResponse {
            email: dto.email,
            name: dto.name,
        })
    }

    pub async fn db_health(&self) -> DbHealthResponse {
        let sample = self
            .prisma
            .query_scalar("SELECT 1")
            .await
            .unwrap_or_else(|e| e);
        DbHealthResponse {
            status: "up".into(),
            sample,
        }
    }

    pub async fn list_users(&self) -> Result<Vec<UserRow>, String> {
        self.prisma
            .query_all_as(r#"SELECT "id", "email", "name" FROM "User""#)
            .await
    }
}

#[dto]
pub struct CreateUserDto {
    #[IsEmail]
    pub email: String,
    #[Length(min = 1, max = 80)]
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct UserResponse {
    pub email: String,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct DbHealthResponse {
    pub status: String,
    pub sample: String,
}

#[controller(prefix = "/api", version = "v1")]
pub struct AppController;

#[routes(state = AppService)]
impl AppController {
    #[get("/")]
    pub async fn root(State(service): State<Arc<AppService>>) -> &'static str {
        service.get_hello()
    }

    #[post("/users")]
    pub async fn create_user(
        State(service): State<Arc<AppService>>,
        ValidatedBody(dto): ValidatedBody<CreateUserDto>,
    ) -> Result<Json<UserResponse>, HttpException> {
        if dto.name.eq_ignore_ascii_case("admin") {
            return Err(ConflictException::new("`admin` is reserved in this demo"));
        }
        match service.create_user(dto).await {
            Ok(user) => Ok(Json(user)),
            Err(e) if e.contains("UNIQUE constraint failed") => Err(ConflictException::new(
                "a user with this email already exists",
            )),
            Err(e) => Err(InternalServerErrorException::new(e)),
        }
    }

    #[get("/db-health")]
    pub async fn db_health(State(service): State<Arc<AppService>>) -> Json<DbHealthResponse> {
        Json(service.db_health().await)
    }

    #[get("/users-db")]
    pub async fn users_db(
        State(service): State<Arc<AppService>>,
    ) -> Result<Json<Vec<UserRow>>, HttpException> {
        service
            .list_users()
            .await
            .map(Json)
            .map_err(InternalServerErrorException::new)
    }

    #[get("/created-style")]
    #[http_code(201)]
    pub async fn created_style() -> &'static str {
        "created-style"
    }

    #[get("/header-style")]
    #[response_header("x-powered-by", "nestrs")]
    pub async fn header_style() -> &'static str {
        "header-style"
    }

    #[get("/docs")]
    #[redirect("https://docs.nestjs.com")]
    pub async fn docs() -> &'static str {
        "docs"
    }

    #[get("/feature")]
    #[ver("v2")]
    pub async fn versioned_feature() -> &'static str {
        "feature-route-v2"
    }
}

#[version("v2")]
#[controller(prefix = "/api")]
pub struct AppControllerV2;

#[routes(state = AppService)]
impl AppControllerV2 {
    #[get("/")]
    pub async fn root() -> &'static str {
        "Hello World v2"
    }
}

#[module(
    imports = [PrismaModule],
    re_exports = [PrismaModule],
)]
pub struct DataModule;

#[module(
    imports = [DataModule],
    controllers = [AppController, AppControllerV2],
    providers = [AppService],
)]
pub struct AppModule;

#[tokio::main]
async fn main() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // `mode=rwc` lets sqlx create the database file on first run instead of failing with
    // "unable to open database file".
    let db_url = format!(
        "sqlite:{}?mode=rwc",
        base.join("dev.db").display()
    );
    let schema_path = base.join("prisma/schema.prisma");

    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url(db_url.clone())
            .pool_min(1)
            .pool_max(10)
            .schema_path(schema_path.to_string_lossy().as_ref()),
    );

    let prisma = PrismaService::default();
    let mut bootstrap_ok = true;
    for ddl in [
        r#"CREATE TABLE IF NOT EXISTS "User" (
            "id" INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
            "email" TEXT NOT NULL,
            "name" TEXT NOT NULL
        )"#,
        r#"CREATE UNIQUE INDEX IF NOT EXISTS "User_email_key" ON "User"("email")"#,
    ] {
        if let Err(e) = prisma.execute(ddl).await {
            eprintln!("hello-app schema bootstrap failed, exiting: {e}");
            bootstrap_ok = false;
            break;
        }
    }
    if !bootstrap_ok {
        std::process::exit(1);
    }

    // Resulting URL layout follows NestJS URI-versioning order:
    //   {global-prefix}/{version}/{controller-prefix}/{route}
    // e.g. this app serves `GET /platform/v1/api/` and `POST /platform/v1/api/users`.
    // To get the common `/platform/api/v1/...` shape instead, put `api` in the global
    // prefix (`set_global_prefix("platform/api")`) and drop it from `#[controller(prefix)]`.
    NestFactory::create::<AppModule>()
        .set_global_prefix("platform")
        .listen_graceful(3000)
        .await;
}
