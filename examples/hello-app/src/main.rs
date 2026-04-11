use nestrs::prelude::*;
use nestrs_prisma::{PrismaModule, PrismaOptions, PrismaService};
use std::sync::Arc;

#[injectable]
pub struct AppService {
    prisma: Arc<PrismaService>,
}

impl AppService {
    pub fn get_hello(&self) -> &'static str {
        "Hello World"
    }

    pub fn create_user(&self, dto: CreateUserDto) -> UserResponse {
        UserResponse {
            email: dto.email,
            name: dto.name,
        }
    }

    pub fn db_health(&self) -> DbHealthResponse {
        DbHealthResponse {
            status: self.prisma.health().to_string(),
            sample: self.prisma.query_raw("select 1"),
        }
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
        Ok(Json(service.create_user(dto)))
    }

    #[get("/db-health")]
    pub async fn db_health(
        State(service): State<Arc<AppService>>,
    ) -> Result<Json<DbHealthResponse>, HttpException> {
        Ok(Json(service.db_health()))
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
    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url("file:./dev.db")
            .pool_min(1)
            .pool_max(10),
    );

    NestFactory::create::<AppModule>()
        .set_global_prefix("platform")
        .listen_graceful(3000)
        .await;
}
