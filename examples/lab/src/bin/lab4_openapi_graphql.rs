//! Lab 4 — OpenAPI + GraphQL: spec-vs-routes drift, Swagger UI, GraphQL query
//! execution, and depth/complexity limit enforcement.
//!
//! Writes two attack queries to /tmp for curl use:
//! - `/tmp/lab4-complex-query.json` (exceeds complexity 512)
//! - `/tmp/lab4-deep-query.json`    (exceeds depth 64)
//! Also exports SDL to `/tmp/lab4-schema.graphql`.
//!
//! Run: `cargo run -p lab --bin lab4_openapi_graphql`

use async_graphql::{EmptyMutation, EmptySubscription, Object, SimpleObject};
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct CatalogService;

#[derive(serde::Deserialize)]
pub struct NameParams {
    name: String,
}

#[derive(serde::Deserialize, validator::Validate)]
pub struct EchoDto {
    #[validate(length(min = 1, max = 20))]
    pub text: String,
}

#[controller(prefix = "/cat", version = "v1")]
pub struct CatalogController;

#[routes(state = CatalogService)]
impl CatalogController {
    /// Greet by name.
    #[openapi(
        summary = "Greet a user",
        tag = "catalog",
        responses = ((200, "greeting text"))
    )]
    #[get("/hello/:name")]
    pub async fn hello(#[param::param] p: NameParams) -> String {
        format!("hello, {}", p.name)
    }

    /// Echo validated body.
    #[openapi(
        summary = "Echo a payload",
        tag = "catalog",
        responses = ((201, "echoed payload"))
    )]
    #[post("/echo")]
    pub async fn echo(ValidatedBody(dto): ValidatedBody<EchoDto>) -> Json<serde_json::Value> {
        Json(serde_json::json!({ "echoed": dto.text }))
    }
}

// ---- GraphQL schema ----

#[derive(SimpleObject, Clone)]
struct User {
    id: i32,
    name: String,
    friends: Vec<User>,
}

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn hello(&self) -> &'static str {
        "world"
    }

    async fn user(&self) -> User {
        // friends intentionally empty: deep client queries parse but terminate.
        User {
            id: 1,
            name: "ada".into(),
            friends: vec![],
        }
    }
}

fn build_schema() -> nestrs::graphql::Schema<QueryRoot, EmptyMutation, EmptySubscription> {
    nestrs::graphql::with_default_limits(async_graphql::Schema::build(
        QueryRoot,
        EmptyMutation,
        EmptySubscription,
    ))
    .finish()
}

fn write_attack_queries() {
    // Complexity attack: >512 field weights via aliases on one document.
    let mut complex = String::from("{\"query\":\"{");
    for i in 0..600 {
        complex.push_str(&format!(" a{i}: hello"));
    }
    complex.push_str("}\"}");

    // Depth attack: 80 nested selections against the recursive User.friends field.
    let levels = 80;
    let mut q = String::from("{ user ");
    for _ in 0..levels {
        q.push_str("{ friends ");
    }
    q.push_str("{ id }");
    for _ in 0..levels {
        q.push_str(" }");
    }
    q.push_str(" }");
    let deep = format!("{{\"query\":{}}}", serde_json::to_string(&q).unwrap());

    std::fs::write("/tmp/lab4-complex-query.json", complex).expect("write complex query");
    std::fs::write("/tmp/lab4-deep-query.json", deep).expect("write deep query");
}

fn export_sdl(schema: &nestrs::graphql::Schema<QueryRoot, EmptyMutation, EmptySubscription>) {
    let sdl = nestrs::graphql::export_schema_sdl(schema);
    std::fs::write("/tmp/lab4-schema.graphql", sdl).expect("write sdl");
}

#[module(controllers = [CatalogController], providers = [CatalogService])]
pub struct LabModule;

#[tokio::main]
async fn main() {
    let schema = build_schema();
    export_sdl(&schema);
    write_attack_queries();

    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .enable_openapi()
        .enable_graphql(schema)
        .listen_graceful(3400)
        .await;
}
