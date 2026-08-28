//! Comprehensive source-parser tests for the 13 introspection tools.
//!
//! Each test exercises a distinct code path inside `introspection::source`
//! (the `syn`-based walker) by feeding the parser a small fixture crate and
//! asserting on what the parser returns. The tests intentionally avoid
//! depending on `nestrs-macros` at compile time (the parser is source-only)
//! so the fixtures use the attribute surface syntactically without
//! requiring the proc-macros to expand.
//!
//! The fixture's source code is the same one we'd write in a real
//! application — `#[controller("/users")]`, `#[routes(UserController)]`,
//! `#[get("/")]`, `#[use_guards(...)]`, etc. — so this also serves as a
//! documentation of the macro surface the parser recognizes.

use std::fs;
use std::path::Path;

use nestrs_mcp::introspection::parse_workspace;

// ---------- fixture helpers ----------

fn write_cargo_toml(root: &Path, name: &str) {
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\nnestrs = \"0.4\"\n"
        ),
    )
    .unwrap();
}

fn write_source(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// The reference fixture: one AppModule, one injectable provider, one
/// controller with routes across every HTTP method, one DTO with
/// validators, plus a scheduled job, event handler, and queue processor.
const COMPREHENSIVE_FIXTURE: &str = r#"
use nestrs::{
    controller, dto, injectable, module, routes, Module,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Default, Module)]
#[module(
    imports = (OtherModule,),
    controllers = (UserController, OrderController),
    providers = (UserService, OrderService),
    exports = (UserService,),
)]
pub struct AppModule;

#[derive(Default, Module)]
#[module(providers = ())]
pub struct OtherModule;

#[derive(Default)]
#[injectable]
pub struct UserService {
    inner: String,
}

#[derive(Default)]
#[injectable(scope = "transient")]
pub struct OrderService;

#[derive(Default)]
#[controller("/users")]
pub struct UserController;

#[routes(UserController, state = AppState, controller_guards = (AuthGuard,))]
impl UserController {
    #[get("/")]
    async fn list(&self) -> Vec<String> { vec![] }

    #[get("/:id")]
    async fn get_one(&self, _id: String) -> String { String::new() }

    #[post("/")]
    async fn create(
        &self,
        _body: nestrs::extractor::ValidatedBody<CreateUserDto>,
        _user: nestrs::extractor::Query<()>,
    ) -> String { String::new() }

    #[put("/:id")]
    async fn update(&self, _id: String) -> String { String::new() }

    #[patch("/:id")]
    async fn patch(&self, _id: String) -> String { String::new() }

    #[delete("/:id")]
    async fn delete(&self, _id: String) -> String { String::new() }

    #[options("/:id")]
    async fn options(&self, _id: String) -> String { String::new() }

    #[head("/:id")]
    async fn head(&self, _id: String) -> String { String::new() }

    #[all("/:id/any")]
    async fn any_method(&self, _id: String) -> String { String::new() }

    #[ver("v2")]
    #[get("/admin")]
    async fn admin(&self) -> String { String::new() }

    #[get("/guarded")]
    #[use_guards(RoleGuard, RateLimitGuard)]
    async fn guarded(&self) -> String { String::new() }

    #[get("/intercepted")]
    #[use_interceptors(LoggingInterceptor)]
    async fn intercepted(&self) -> String { String::new() }

    #[get("/piped")]
    #[use_pipes(TrimPipe)]
    async fn piped(&self) -> String { String::new() }

    #[get("/filtered")]
    #[use_filters(AllExceptionsFilter)]
    async fn filtered(&self) -> String { String::new() }

    #[get("/meta")]
    #[set_metadata("feature_flag", "experimental")]
    #[roles("admin", "ops")]
    async fn meta(&self) -> String { String::new() }

    #[get("/openapi")]
    #[openapi(summary = "List users", operation_id = "listUsers")]
    async fn openapi_doc(&self) -> String { String::new() }
}

#[derive(Default)]
#[controller("/orders")]
pub struct OrderController;

#[routes(OrderController)]
impl OrderController {
    #[get("/")]
    async fn list_orders(&self) -> Vec<String> { vec![] }
}

#[derive(Debug, Serialize, Deserialize, Validate)]
#[dto]
pub struct CreateUserDto {
    #[IsString]
    #[MinLength(1)]
    #[MaxLength(100)]
    pub name: String,
    #[IsEmail]
    pub email: String,
    #[IsUrl]
    pub website: String,
    #[Min(0)]
    #[Max(150)]
    pub age: i32,
    #[Matches(r"^[a-z0-9_]+$")]
    pub username: String,
    #[ValidateNested]
    pub address: AddressDto,
    #[IsOptional]
    pub bio: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Validate)]
#[dto(allow_unknown_fields, expose_only)]
pub struct AddressDto {
    pub street: String,
    pub city: String,
}

#[derive(Default)]
pub struct AppState;

// Top-level scheduled / event / queue handlers — these are
// module-level fns, not inside an impl block, and are caught by
// `Item::Fn` in the visitor.
#[interval(seconds = 60)]
pub async fn tick() {}

#[cron("0 * * * * *")]
pub async fn hourly() {}

#[on_event("user.created")]
pub async fn on_user_created() {}

#[process("emails")]
pub async fn process_email(_msg: String) {}
"#;

/// Set up a fresh fixture workspace and return its root.
fn make_workspace() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    write_cargo_toml(tmp.path(), "fixture");
    write_source(tmp.path(), "src/lib.rs", COMPREHENSIVE_FIXTURE);
    tmp
}

// ---------- tool-by-tool coverage ----------

#[test]
fn list_modules_returns_both_module_structs() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).expect("parse should succeed");
    let names: Vec<&str> = parsed.modules.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"AppModule"), "AppModule missing: {names:?}");
    assert!(names.contains(&"OtherModule"), "OtherModule missing: {names:?}");

    let app = parsed.modules.iter().find(|m| m.name == "AppModule").unwrap();
    assert!(app.controllers.contains(&"UserController".to_string()));
    assert!(app.controllers.contains(&"OrderController".to_string()));
    assert!(app.providers.contains(&"UserService".to_string()));
    assert!(app.exports.contains(&"UserService".to_string()));
    assert!(app.imports.contains(&"OtherModule".to_string()));
}

#[test]
fn get_module_returns_one_by_name() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let app = parsed
        .modules
        .iter()
        .find(|m| m.name == "AppModule")
        .expect("AppModule");
    assert_eq!(app.name, "AppModule");
    assert_eq!(app.controllers.len(), 2);
    assert_eq!(app.providers.len(), 2);
    assert_eq!(app.exports.len(), 1);
    assert_eq!(app.imports.len(), 1);
}

#[test]
fn list_controllers_returns_both_with_prefixes() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let names: Vec<&str> = parsed.controllers.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"UserController"));
    assert!(names.contains(&"OrderController"));

    let user = parsed
        .controllers
        .iter()
        .find(|c| c.name == "UserController")
        .unwrap();
    assert_eq!(user.prefix.as_deref(), Some("/users"));
    assert!(user.version.is_none(), "no kv form was set on this controller");
    assert!(user.host.is_none());
    assert_eq!(user.controller_guards, vec!["AuthGuard".to_string()]);
    assert_eq!(user.state.as_deref(), Some("AppState"));

    let order = parsed
        .controllers
        .iter()
        .find(|c| c.name == "OrderController")
        .unwrap();
    assert_eq!(order.prefix.as_deref(), Some("/orders"));
    assert!(order.version.is_none());
    assert!(order.controller_guards.is_empty());
}

#[test]
fn get_controller_returns_routes_guards_and_state() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let user = parsed
        .controllers
        .iter()
        .find(|c| c.name == "UserController")
        .unwrap();
    assert_eq!(user.routes.len(), 16, "expected all 16 routes parsed, got {}", user.routes.len());
    let methods: Vec<&str> = user.routes.iter().map(|r| r.method.as_str()).collect();
    for m in &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"] {
        assert!(methods.contains(m), "missing HTTP method {m} in {methods:?}");
    }
    assert!(methods.contains(&"ALL"), "ALL method missing: {methods:?}");
}

#[test]
fn list_providers_returns_both_with_scope() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let user = parsed
        .providers
        .iter()
        .find(|p| p.type_name == "UserService")
        .unwrap();
    assert!(user.is_injectable);
    // Bare `#[injectable]` is recorded as `scope = "singleton"` (the
    // default), NOT `None`. The parser is conservative — it always
    // reports a scope when `#[injectable]` is present.
    assert_eq!(user.scope.as_deref(), Some("singleton"));

    let order = parsed
        .providers
        .iter()
        .find(|p| p.type_name == "OrderService")
        .unwrap();
    assert!(order.is_injectable);
    assert_eq!(order.scope.as_deref(), Some("transient"));
}

#[test]
fn get_provider_returns_constructor_signature() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let user = parsed
        .providers
        .iter()
        .find(|p| p.type_name == "UserService")
        .unwrap();
    assert_eq!(user.type_name, "UserService");
    assert!(user.is_injectable);
}

#[test]
fn list_routes_returns_routes_from_all_controllers() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let user = parsed
        .controllers
        .iter()
        .find(|c| c.name == "UserController")
        .unwrap();
    assert!(user.routes.len() >= 16, "user has {} routes", user.routes.len());
    let order = parsed
        .controllers
        .iter()
        .find(|c| c.name == "OrderController")
        .unwrap();
    assert!(!order.routes.is_empty(), "order has {} routes", order.routes.len());
    assert!(order.routes.iter().any(|r| r.method == "GET" && r.path == "/"));
}

#[test]
fn get_route_finds_by_method_and_path() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let user = parsed
        .controllers
        .iter()
        .find(|c| c.name == "UserController")
        .unwrap();
    let create = user
        .routes
        .iter()
        .find(|r| r.method == "POST" && r.path == "/")
        .expect("POST / route missing");
    assert_eq!(create.handler, "create");
    assert!(
        create.body_type.as_deref().map(|s| s.contains("ValidatedBody")).unwrap_or(false),
        "POST body_type should mention ValidatedBody, got: {:?}",
        create.body_type
    );
    assert!(
        create.response_type.is_some(),
        "POST response_type should be populated"
    );

    let admin = user
        .routes
        .iter()
        .find(|r| r.method == "GET" && r.path == "/admin")
        .expect("GET /admin missing");
    assert_eq!(admin.version.as_deref(), Some("v2"), "ver override captured");

    let guarded = user
        .routes
        .iter()
        .find(|r| r.handler == "guarded")
        .expect("guarded route missing");
    assert_eq!(guarded.guards, vec!["RoleGuard".to_string(), "RateLimitGuard".to_string()]);

    let intercepted = user
        .routes
        .iter()
        .find(|r| r.handler == "intercepted")
        .unwrap();
    assert_eq!(intercepted.interceptors, vec!["LoggingInterceptor".to_string()]);

    let piped = user.routes.iter().find(|r| r.handler == "piped").unwrap();
    assert_eq!(piped.pipes, vec!["TrimPipe".to_string()]);

    let filtered = user.routes.iter().find(|r| r.handler == "filtered").unwrap();
    assert_eq!(filtered.filters, vec!["AllExceptionsFilter".to_string()]);

    let meta = user.routes.iter().find(|r| r.handler == "meta").unwrap();
    assert_eq!(meta.metadata.get("feature_flag").map(|s| s.as_str()), Some("experimental"));
    assert_eq!(meta.metadata.get("roles").map(|s| s.as_str()), Some("admin,ops"));

    let op = user.routes.iter().find(|r| r.handler == "openapi_doc").unwrap();
    assert_eq!(op.metadata.get("openapi.summary").map(|s| s.as_str()), Some("List users"));
    assert_eq!(op.metadata.get("openapi.operation_id").map(|s| s.as_str()), Some("listUsers"));
}

#[test]
fn list_dtos_returns_both_with_field_counts() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let names: Vec<&str> = parsed.dtos.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"CreateUserDto"));
    assert!(names.contains(&"AddressDto"));

    let create = parsed.dtos.iter().find(|d| d.name == "CreateUserDto").unwrap();
    assert_eq!(create.field_count, 7, "name + email + website + age + username + address + bio = 7");

    let addr = parsed.dtos.iter().find(|d| d.name == "AddressDto").unwrap();
    assert!(addr.allow_unknown_fields);
    assert!(addr.expose_only);
}

#[test]
fn get_dto_returns_one_by_name() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let create = parsed
        .dtos
        .iter()
        .find(|d| d.name == "CreateUserDto")
        .expect("CreateUserDto");
    assert_eq!(create.name, "CreateUserDto");
    assert_eq!(create.field_count, 7);
    assert!(!create.allow_unknown_fields);
    assert!(!create.expose_only);
}

#[test]
fn list_schedules_catches_interval_and_cron() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let names: Vec<&str> = parsed.schedules.iter().map(|s| s.as_str()).collect();
    assert!(names.iter().any(|s| s.contains("interval::tick")), "interval::tick in {names:?}");
    assert!(names.iter().any(|s| s.contains("cron::hourly")), "cron::hourly in {names:?}");
}

#[test]
fn list_event_handlers_catches_on_event() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let names: Vec<&str> = parsed.event_handlers.iter().map(|s| s.as_str()).collect();
    assert!(
        names.iter().any(|s| s.contains("on_event::on_user_created")),
        "on_event::on_user_created in {names:?}"
    );
}

#[test]
fn list_queue_processors_catches_process() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    let names: Vec<&str> = parsed.queue_processors.iter().map(|s| s.as_str()).collect();
    assert!(
        names.iter().any(|s| s.contains("process::process_email")),
        "process::process_email in {names:?}"
    );
}

// ---------- error path coverage ----------

#[test]
fn parse_errors_on_missing_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let result = parse_workspace(tmp.path());
    let err = result.expect_err("should error on no Cargo.toml");
    let msg = format!("{err:?}");
    assert!(msg.contains("Cargo.toml") || msg.contains("workspace"), "got: {msg}");
}

#[test]
fn parse_errors_on_nonexistent_workspace() {
    let result = parse_workspace(Path::new("/this/path/does/not/exist/anywhere"));
    let err = result.expect_err("should error on nonexistent path");
    let msg = format!("{err}");
    assert!(msg.contains("does not exist") || msg.contains("not found"), "got: {msg}");
}

#[test]
fn controller_kv_form_extracts_prefix_version_host() {
    // The `#[controller(prefix = "/api", version = "v2", host = "...")]`
    // kv form is documented separately from the bare-string form.
    let tmp = tempfile::tempdir().unwrap();
    write_cargo_toml(tmp.path(), "kv-form");
    write_source(
        tmp.path(),
        "src/lib.rs",
        r#"
use nestrs::{controller, routes};
#[derive(Default)]
#[controller(prefix = "/api", version = "v2", host = "api.example.com")]
pub struct ApiController;
#[routes(ApiController)]
impl ApiController {
    #[get("/health")]
    async fn health(&self) -> String { "ok".into() }
}
"#,
    );
    let parsed = parse_workspace(tmp.path()).expect("parse should succeed");
    let api = parsed
        .controllers
        .iter()
        .find(|c| c.name == "ApiController")
        .expect("ApiController");
    assert_eq!(api.prefix.as_deref(), Some("/api"));
    assert_eq!(api.version.as_deref(), Some("v2"));
    assert_eq!(api.host.as_deref(), Some("api.example.com"));
    assert_eq!(api.routes.len(), 1);
    assert_eq!(api.routes[0].method, "GET");
    assert_eq!(api.routes[0].path, "/health");
}

#[test]
fn parse_succeeds_with_no_src_dir() {
    // `scaffold::new_project` needs this: an empty workspace with no
    // `src/` should parse cleanly (zero results) so the model can preview
    // a brand-new crate.
    let tmp = tempfile::tempdir().unwrap();
    write_cargo_toml(tmp.path(), "empty-fixture");
    let parsed = parse_workspace(tmp.path()).expect("empty workspace should parse");
    assert_eq!(parsed.modules.len(), 0);
    assert_eq!(parsed.controllers.len(), 0);
    assert_eq!(parsed.stats.files_scanned, 0);
}

#[test]
fn unknown_module_arg_does_not_fail_parse() {
    let tmp = tempfile::tempdir().unwrap();
    write_cargo_toml(tmp.path(), "future");
    write_source(
        tmp.path(),
        "src/lib.rs",
        r#"
use nestrs::{module, Module};
#[derive(Default, Module)]
#[module(some_future_attr = "ignored")]
pub struct AppModule;
"#,
    );
    let parsed = parse_workspace(tmp.path()).expect("parse should not fail");
    assert_eq!(parsed.modules.len(), 1);
    // The unknown arg should show up as a warning.
    let has_warning = parsed
        .warnings
        .iter()
        .any(|w| w.kind == "module" && w.message.contains("unrecognized"));
    assert!(has_warning, "expected an unrecognized module warning, got: {:?}", parsed.warnings);
}

#[test]
fn route_with_use_guards_only_emits_warning() {
    // A method that has `#[use_guards(...)]` but no `#[get/post/...]`
    // should NOT become a route, but should emit a warning so the model
    // can flag the dangling decorator.
    let tmp = tempfile::tempdir().unwrap();
    write_cargo_toml(tmp.path(), "dangling");
    write_source(
        tmp.path(),
        "src/lib.rs",
        r#"
use nestrs::{controller, routes};
#[derive(Default)]
#[controller("/x")]
pub struct C;
#[routes(C)]
impl C {
    #[use_guards(G)]
    fn dangling(&self) {}
}
"#,
    );
    let parsed = parse_workspace(tmp.path()).expect("parse should not fail");
    let c = parsed.controllers.iter().find(|c| c.name == "C").unwrap();
    assert!(
        c.routes.is_empty(),
        "use_guards without an HTTP method should not produce a route, got: {:?}",
        c.routes
    );
    let warn = parsed
        .warnings
        .iter()
        .find(|w| w.kind == "routes" && w.message.contains("dangling"))
        .expect("expected warning for dangling decorator");
    assert!(warn.message.contains("dangling"));
}

// ---------- stats sanity ----------

#[test]
fn stats_reflect_workspace_shape() {
    let tmp = make_workspace();
    let parsed = parse_workspace(tmp.path()).unwrap();
    assert!(parsed.stats.files_scanned >= 1);
    assert_eq!(parsed.stats.modules, 2);
    assert_eq!(parsed.stats.controllers, 2);
    assert_eq!(parsed.stats.providers, 2);
    assert_eq!(parsed.stats.dtos, 2);
    assert!(parsed.stats.routes >= 17, "expected >= 17 routes, got {}", parsed.stats.routes);
    assert!(parsed.stats.warnings == 0, "expected no warnings, got {}", parsed.stats.warnings);
    assert_eq!(parsed.schedules.len(), 2);
    assert_eq!(parsed.event_handlers.len(), 1);
    assert_eq!(parsed.queue_processors.len(), 1);
}
