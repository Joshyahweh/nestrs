//! Full CRUD-oriented scaffolds for `nestrs generate resource <name> --transport ...`.

/// Workspace / scaffold package version — keep `nestrs` dependency version aligned in generated hints.
pub const SCAFFOLD_NESTRS_VERSION_HINT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
pub enum ResourceTransport {
    Rest,
    Graphql,
    Ws,
    Grpc,
    Microservice,
}

pub fn crud_service(pascal: &str) -> String {
    format!(
        r#"use nestrs::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// JSON / HTTP / message payload view of a `{pascal}` row.
#[derive(Clone, Serialize)]
pub struct {pascal}Response {{
    pub id: u64,
    pub name: String,
}}

/// In-memory CRUD store (swap for a real database in production).
#[injectable]
pub struct {pascal}Service {{
    store: Arc<RwLock<HashMap<u64, {pascal}Response>>>,
    next_id: Arc<RwLock<u64>>,
}}

impl Default for {pascal}Service {{
    fn default() -> Self {{
        Self {{
            store: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(RwLock::new(1)),
        }}
    }}
}}

impl {pascal}Service {{
    pub async fn list(&self) -> Vec<{pascal}Response> {{
        let g = self.store.read().await;
        g.values().cloned().collect()
    }}

    pub async fn get(&self, id: u64) -> Option<{pascal}Response> {{
        self.store.read().await.get(&id).cloned()
    }}

    pub async fn create(&self, name: String) -> {pascal}Response {{
        let mut idg = self.next_id.write().await;
        let id = *idg;
        *idg += 1;
        let row = {pascal}Response {{ id, name }};
        self.store.write().await.insert(id, row.clone());
        row
    }}

    pub async fn update(&self, id: u64, name: String) -> Result<{pascal}Response, HttpException> {{
        let mut g = self.store.write().await;
        let entry = g
            .get_mut(&id)
            .ok_or_else(|| NotFoundException::new("not found"))?;
        entry.name = name;
        Ok(entry.clone())
    }}

    pub async fn delete(&self, id: u64) -> Result<(), HttpException> {{
        self.store
            .write()
            .await
            .remove(&id)
            .ok_or_else(|| NotFoundException::new("not found"))?;
        Ok(())
    }}
}}
"#,
        pascal = pascal,
    )
}

pub fn rest_controller(snake: &str, pascal: &str) -> String {
    let ver = SCAFFOLD_NESTRS_VERSION_HINT;
    let use_service = format!(
        "use super::{}_service::{{{pascal}Response, {pascal}Service}};",
        snake,
        pascal = pascal
    );
    format!(
        r#"//! REST CRUD for `{pascal}`.
//! ## OpenAPI (optional)
//! Add **`openapi`** to **`nestrs`** in **Cargo.toml** (pulls **`nestrs-openapi`**):  
//! `nestrs = {{ version = "{ver}", features = ["openapi"] }}`  
//! Then chain **`.enable_openapi()`** (or **`.enable_openapi_with_options(nestrs::nestrs_openapi::OpenApiOptions {{ ... }})`**) on **`NestFactory`**.  
//! The **`#[openapi(...)]`** attributes below register summaries/tags/responses for **`/openapi.json`**.

use nestrs::prelude::*;
use std::sync::Arc;
{use_service}

#[dto]
pub struct {pascal}IdParam {{
    #[validate(range(min = 1))]
    pub id: u64,
}}

#[controller(prefix = "/{snake}")]
pub struct {pascal}Controller;

#[routes(state = {pascal}Service)]
impl {pascal}Controller {{
    #[openapi(summary = "List {pascal} rows", tag = "{snake}", responses = ((200, "OK")))]
    #[get("/")]
    #[serialize]
    async fn list(State(s): State<Arc<{pascal}Service>>) -> Vec<{pascal}Response> {{
        s.list().await
    }}

    #[openapi(
        summary = "Get {pascal} by id",
        tag = "{snake}",
        responses = ((200, "Found"), (404, "Not found"))
    )]
    #[get("/:id")]
    #[serialize]
    #[use_pipes(ValidationPipe)]
    async fn get_one(
        State(s): State<Arc<{pascal}Service>>,
        #[param::param] p: {pascal}IdParam,
    ) -> Result<{pascal}Response, HttpException> {{
        s.get(p.id)
            .await
            .ok_or_else(|| NotFoundException::new("not found"))
    }}

    #[openapi(summary = "Create {pascal}", tag = "{snake}", responses = ((200, "Created")))]
    #[post("/")]
    #[serialize]
    #[use_pipes(ValidationPipe)]
    async fn create(
        State(s): State<Arc<{pascal}Service>>,
        #[param::body] body: super::create_{snake}_dto::Create{pascal}Dto,
    ) -> {pascal}Response {{
        s.create(body.name).await
    }}

    #[openapi(
        summary = "Update {pascal}",
        tag = "{snake}",
        responses = ((200, "Updated"), (404, "Not found"))
    )]
    #[put("/:id")]
    #[serialize]
    #[use_pipes(ValidationPipe)]
    async fn update(
        State(s): State<Arc<{pascal}Service>>,
        #[param::param] p: {pascal}IdParam,
        #[param::body] body: super::update_{snake}_dto::Update{pascal}Dto,
    ) -> Result<{pascal}Response, HttpException> {{
        s.update(p.id, body.name).await
    }}

    #[openapi(
        summary = "Delete {pascal}",
        tag = "{snake}",
        responses = ((200, "Deleted"), (404, "Not found"))
    )]
    #[delete("/:id")]
    #[serialize]
    #[use_pipes(ValidationPipe)]
    async fn delete(
        State(s): State<Arc<{pascal}Service>>,
        #[param::param] p: {pascal}IdParam,
    ) -> Result<&'static str, HttpException> {{
        s.delete(p.id).await?;
        Ok("deleted")
    }}
}}
"#,
        snake = snake,
        pascal = pascal,
        use_service = use_service,
        ver = ver,
    )
}

pub fn graphql_resolver(snake: &str, pascal: &str) -> String {
    let use_service = format!(
        "use super::{}_service::{{{pascal}Response, {pascal}Service}};",
        snake,
        pascal = pascal
    );
    format!(
        r#"//! GraphQL CRUD scaffold for `{pascal}`.
//! Enable in `Cargo.toml`:
//!   nestrs = {{ version = "0.3.3", features = ["graphql"] }}
//!   async-graphql = "=7.0.17"
//!
//! In `main`, resolve `Arc<{pascal}Service>` from your module graph, then:
//!   use async_graphql::{{EmptySubscription, Schema}};
//!   let schema = Schema::build({pascal}GqlQuery::default(), {pascal}GqlMutation::default(), EmptySubscription)
//!       .data(service.clone())
//!       .finish();
//!   app.enable_graphql(schema);

use async_graphql::{{Context, Object, SimpleObject}};
use nestrs::prelude::*;
use std::sync::Arc;
{use_service}

#[derive(SimpleObject, Clone)]
pub struct {pascal}GqlRow {{
    pub id: u64,
    pub name: String,
}}

impl From<{pascal}Response> for {pascal}GqlRow {{
    fn from(r: {pascal}Response) -> Self {{
        Self {{ id: r.id, name: r.name }}
    }}
}}

#[derive(Default)]
pub struct {pascal}GqlQuery;

#[Object]
impl {pascal}GqlQuery {{
    async fn {snake}_list(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<{pascal}GqlRow>> {{
        let s = ctx.data::<Arc<{pascal}Service>>()?;
        Ok(s.list().await.into_iter().map(Into::into).collect())
    }}

    async fn {snake}_get(&self, ctx: &Context<'_>, id: u64) -> async_graphql::Result<Option<{pascal}GqlRow>> {{
        let s = ctx.data::<Arc<{pascal}Service>>()?;
        Ok(s.get(id).await.map(Into::into))
    }}
}}

#[derive(Default)]
pub struct {pascal}GqlMutation;

#[Object]
impl {pascal}GqlMutation {{
    async fn {snake}_create(&self, ctx: &Context<'_>, name: String) -> async_graphql::Result<{pascal}GqlRow> {{
        let s = ctx.data::<Arc<{pascal}Service>>()?;
        Ok(s.create(name).await.into())
    }}

    async fn {snake}_update(
        &self,
        ctx: &Context<'_>,
        id: u64,
        name: String,
    ) -> async_graphql::Result<{pascal}GqlRow> {{
        let s = ctx.data::<Arc<{pascal}Service>>()?;
        s.update(id, name)
            .await
            .map_err(|e| async_graphql::Error::new(e.message.clone()))
            .map(Into::into)
    }}

    async fn {snake}_delete(&self, ctx: &Context<'_>, id: u64) -> async_graphql::Result<bool> {{
        let s = ctx.data::<Arc<{pascal}Service>>()?;
        s.delete(id)
            .await
            .map_err(|e| async_graphql::Error::new(e.message.clone()))?;
        Ok(true)
    }}
}}
"#,
        snake = snake,
        pascal = pascal,
        use_service = use_service,
    )
}

pub fn ws_gateway(snake: &str, pascal: &str) -> String {
    let use_service = format!(
        "use super::{}_service::{pascal}Service;",
        snake,
        pascal = pascal
    );
    format!(
        r#"//! WebSocket CRUD-style messages for `{pascal}` (JSON payloads).
//! Enable: `nestrs = {{ version = "0.3.3", features = ["ws"] }}`
//!
//! Client frames use `{{ "event": "...", "data": {{ ... }} }}` (see `nestrs_ws::WsEvent`).

use nestrs::prelude::*;
use std::sync::Arc;
{use_service}

#[ws_gateway(path = "/{snake}/ws")]
#[injectable]
pub struct {pascal}Gateway {{
    service: Arc<{pascal}Service>,
}}

#[ws_routes]
impl {pascal}Gateway {{
    #[subscribe_message("{snake}.list")]
    async fn list(&self, client: nestrs::ws::WsClient, _payload: serde_json::Value) {{
        let rows = self.service.list().await;
        let _ = client.emit("{snake}.list.ok", rows);
    }}

    #[subscribe_message("{snake}.get")]
    async fn get_one(&self, client: nestrs::ws::WsClient, payload: serde_json::Value) {{
        let id = payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let body = match self.service.get(id).await {{
            Some(row) => serde_json::json!({{ "ok": true, "item": row }}),
            None => serde_json::json!({{ "ok": false, "error": "not_found" }}),
        }};
        let _ = client.emit_json("{snake}.get.ok", body);
    }}

    #[subscribe_message("{snake}.create")]
    async fn create(&self, client: nestrs::ws::WsClient, payload: serde_json::Value) {{
        let name = payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let row = self.service.create(name).await;
        let _ = client.emit("{snake}.create.ok", row);
    }}

    #[subscribe_message("{snake}.update")]
    async fn update(&self, client: nestrs::ws::WsClient, payload: serde_json::Value) {{
        let id = payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let name = payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let body = match self.service.update(id, name).await {{
            Ok(row) => serde_json::json!({{ "ok": true, "item": row }}),
            Err(e) => serde_json::json!({{ "ok": false, "error": e.message }}),
        }};
        let _ = client.emit_json("{snake}.update.ok", body);
    }}

    #[subscribe_message("{snake}.delete")]
    async fn delete(&self, client: nestrs::ws::WsClient, payload: serde_json::Value) {{
        let id = payload.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let body = match self.service.delete(id).await {{
            Ok(()) => serde_json::json!({{ "ok": true }}),
            Err(e) => serde_json::json!({{ "ok": false, "error": e.message }}),
        }};
        let _ = client.emit_json("{snake}.delete.ok", body);
    }}
}}
"#,
        snake = snake,
        pascal = pascal,
        use_service = use_service,
    )
}

pub fn microservice_transport(snake: &str, pascal: &str, grpc: bool) -> String {
    let header = if grpc {
        format!(
            "//! gRPC microservice message patterns for `{pascal}`.\n//! Enable `nestrs` features **`microservices`** + **`microservices-grpc`**.\n//! Bootstrap with `NestFactory::create_microservice_grpc::<RootModule>(...)`.\n//!\n",
            pascal = pascal
        )
    } else {
        format!(
            "//! TCP microservice message patterns for `{pascal}`.\n//! Enable `nestrs` feature **`microservices`**.\n//! Bootstrap with `NestFactory::create_microservice::<RootModule>(...)`.\n//!\n",
            pascal = pascal
        )
    };
    let use_service = format!(
        "use super::{}_service::{{{pascal}Response, {pascal}Service}};",
        snake,
        pascal = pascal
    );
    format!(
        r#"{header}
use nestrs::prelude::*;
use std::sync::Arc;
{use_service}

#[dto]
struct {pascal}GetReq {{
    #[validate(range(min = 1))]
    id: u64,
}}

#[dto]
struct {pascal}DeleteReq {{
    #[validate(range(min = 1))]
    id: u64,
}}

#[dto]
struct {pascal}UpdateMsgReq {{
    #[validate(range(min = 1))]
    id: u64,
    #[IsString]
    #[Length(min = 1, max = 120)]
    name: String,
}}

/// HTTP health routes + request/reply message patterns for `{pascal}`.
#[controller(prefix = "/{snake}")]
pub struct {pascal}HttpController;

#[routes(state = {pascal}Service)]
impl {pascal}HttpController {{
    #[get("/health")]
    async fn health() -> &'static str {{
        "ok"
    }}
}}

#[injectable]
pub struct {pascal}Transport {{
    service: Arc<{pascal}Service>,
}}

#[micro_routes]
impl {pascal}Transport {{
    #[message_pattern("{snake}.list")]
    async fn list(&self) -> Result<Vec<{pascal}Response>, HttpException> {{
        Ok(self.service.list().await)
    }}

    #[message_pattern("{snake}.get")]
    async fn get(&self, req: {pascal}GetReq) -> Result<{pascal}Response, HttpException> {{
        self.service
            .get(req.id)
            .await
            .ok_or_else(|| NotFoundException::new("not found"))
    }}

    #[message_pattern("{snake}.create")]
    async fn create(
        &self,
        body: super::create_{snake}_dto::Create{pascal}Dto,
    ) -> Result<{pascal}Response, HttpException> {{
        Ok(self.service.create(body.name).await)
    }}

    #[message_pattern("{snake}.update")]
    async fn update(&self, req: {pascal}UpdateMsgReq) -> Result<{pascal}Response, HttpException> {{
        self.service.update(req.id, req.name).await
    }}

    #[message_pattern("{snake}.delete")]
    async fn delete(&self, req: {pascal}DeleteReq) -> Result<(), HttpException> {{
        self.service.delete(req.id).await
    }}
}}
"#,
        header = header,
        snake = snake,
        pascal = pascal,
        use_service = use_service,
    )
}

pub fn resource_module(transport: ResourceTransport, pascal: &str) -> String {
    match transport {
        ResourceTransport::Rest => format!(
            r#"use nestrs::prelude::*;

#[module(
    controllers = [{pascal}Controller],
    providers = [{pascal}Service],
)]
pub struct {pascal}Module;
"#,
            pascal = pascal,
        ),
        ResourceTransport::Graphql => format!(
            r#"use nestrs::prelude::*;

#[module(
    providers = [{pascal}Service],
)]
pub struct {pascal}Module;
"#,
            pascal = pascal,
        ),
        ResourceTransport::Ws => format!(
            r#"use nestrs::prelude::*;

#[module(
    controllers = [{pascal}Gateway],
    providers = [{pascal}Service, {pascal}Gateway],
)]
pub struct {pascal}Module;
"#,
            pascal = pascal,
        ),
        ResourceTransport::Grpc | ResourceTransport::Microservice => format!(
            r#"use nestrs::prelude::*;

#[module(
    controllers = [{pascal}HttpController],
    providers = [{pascal}Service, {pascal}Transport],
    microservices = [{pascal}Transport],
)]
pub struct {pascal}Module;
"#,
            pascal = pascal,
        ),
    }
}

pub fn create_dto(pascal: &str) -> String {
    format!(
        r#"use nestrs::prelude::*;

#[dto]
pub struct Create{pascal}Dto {{
    #[IsString]
    #[Length(min = 1, max = 120)]
    pub name: String,
}}
"#,
        pascal = pascal,
    )
}

pub fn update_dto(pascal: &str) -> String {
    format!(
        r#"use nestrs::prelude::*;

#[dto]
pub struct Update{pascal}Dto {{
    #[IsString]
    #[Length(min = 1, max = 120)]
    pub name: String,
}}
"#,
        pascal = pascal,
    )
}
