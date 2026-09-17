//! Wave 7.10 — `nestrs-cli repl graph|routes|providers|dtos` integration
//! tests. Each test writes a small synthetic crate to a temp dir,
//! invokes the in-process `repl` module against that dir, and asserts
//! the parsed graph matches expectations.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use nestrs_scaffold::repl;

fn unique_tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("nestrs-cli-repl-{name}-{nanos}"))
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, body).expect("write");
}

// ---------------------------------------------------------------------------
// Test fixture — a tiny nestrs-shaped crate
// ---------------------------------------------------------------------------

const USERS_DTO: &str = r#"
use nestrs::prelude::*;

#[dto]
pub struct CreateUserDto {
    #[IsString]
    pub name: String,
}

#[nestrs::partial_type]
pub struct UpdateUserDto {
    #[IsString]
    pub name: String,
}
"#;

const USERS_SERVICE: &str = r#"
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
pub struct UsersService;

impl UsersService {
    pub async fn list(&self) -> Vec<String> {
        Vec::new()
    }
}
"#;

const USERS_CONTROLLER: &str = r#"
use nestrs::prelude::*;
use super::dto::{CreateUserDto, UpdateUserDto};
use super::service::UsersService;

#[controller(prefix = "/users")]
pub struct UsersController;

impl UsersController {
    #[get("/")]
    pub async fn list(svc: UsersService) -> Json<Vec<String>> {
        Json(svc.list().await)
    }

    #[post("/")]
    #[http_code(201)]
    pub async fn create(
        ValidatedBody(input): ValidatedBody<CreateUserDto>,
    ) -> Json<String> {
        Json(input.name)
    }

    #[patch("/:id")]
    pub async fn update(
        PathParam(id): PathParam<String>,
        ValidatedBody(input): ValidatedBody<UpdateUserDto>,
    ) -> Json<String> {
        let _ = (id, input.name);
        Json("ok".to_string())
    }
}
"#;

const USERS_MODULE: &str = r#"
use nestrs::prelude::*;

#[module(
    controllers = [UsersController],
    providers = [UsersService],
    exports = [UsersService],
)]
pub struct UsersModule;
"#;

const APP_MODULE: &str = r#"
use nestrs::prelude::*;

#[module(
    imports = [UsersModule],
    controllers = [],
    providers = [],
)]
pub struct AppModule;
"#;

const USERS_MOD: &str = r#"
pub mod controller;
pub mod dto;
pub mod module;
pub mod service;
"#;

const APP_CONTROLLER: &str = r#"
use nestrs::prelude::*;

#[controller(prefix = "/")]
pub struct AppController;

impl AppController {
    #[get("/")]
    pub async fn root() -> &'static str { "ok" }
    #[get("/health")]
    pub async fn health() -> &'static str { "ok" }
}
"#;

const APP_SERVICE: &str = r#"
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
pub struct AppService;

impl AppService {
    pub fn name(&self) -> &'static str { "app" }
}
"#;

const LIB_RS: &str = r#"
pub mod app;
pub mod users;
"#;

const APP_MOD: &str = r#"
pub mod controller;
pub mod service;
"#;

fn write_fixture(root: &Path) {
    write_file(&root.join("src/lib.rs"), LIB_RS);

    write_file(&root.join("src/app/mod.rs"), APP_MOD);
    write_file(&root.join("src/app/controller.rs"), APP_CONTROLLER);
    write_file(&root.join("src/app/service.rs"), APP_SERVICE);
    write_file(&root.join("src/app/module.rs"), APP_MODULE);

    write_file(&root.join("src/users/mod.rs"), USERS_MOD);
    write_file(&root.join("src/users/dto.rs"), USERS_DTO);
    write_file(&root.join("src/users/service.rs"), USERS_SERVICE);
    write_file(&root.join("src/users/controller.rs"), USERS_CONTROLLER);
    write_file(&root.join("src/users/module.rs"), USERS_MODULE);
}

// ---------------------------------------------------------------------------
// graph
// ---------------------------------------------------------------------------

#[test]
fn graph_finds_modules_with_imports_controllers_providers() {
    let root = unique_tmp_dir("graph");
    write_fixture(&root);

    let g = repl::scan(&root.join("src")).expect("scan");

    let names: Vec<&str> = g.modules.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"UsersModule"), "UsersModule in {names:?}");
    assert!(names.contains(&"AppModule"), "AppModule in {names:?}");

    let users = g.modules.iter().find(|m| m.name == "UsersModule").unwrap();
    assert_eq!(users.controllers, vec!["UsersController".to_string()]);
    assert_eq!(users.providers, vec!["UsersService".to_string()]);

    let app = g.modules.iter().find(|m| m.name == "AppModule").unwrap();
    assert_eq!(app.imports, vec!["UsersModule".to_string()]);
}

#[test]
fn graph_finds_controllers_with_routes() {
    let root = unique_tmp_dir("graph-routes");
    write_fixture(&root);

    let g = repl::scan(&root.join("src")).expect("scan");

    let users = g
        .controllers
        .iter()
        .find(|c| c.name == "UsersController")
        .expect("UsersController present");
    assert_eq!(users.prefix.as_deref(), Some("/users"));

    let methods: Vec<&str> = users.routes.iter().map(|r| r.method.as_str()).collect();
    assert!(methods.contains(&"GET"), "GET in {methods:?}");
    assert!(methods.contains(&"POST"), "POST in {methods:?}");
    assert!(methods.contains(&"PATCH"), "PATCH in {methods:?}");

    let get_root = users.routes.iter().find(|r| r.method == "GET").unwrap();
    assert_eq!(get_root.path, "/");
    assert_eq!(get_root.handler, "list");
}

#[test]
fn graph_finds_injectable_providers() {
    let root = unique_tmp_dir("graph-providers");
    write_fixture(&root);

    let g = repl::scan(&root.join("src")).expect("scan");
    let names: Vec<&str> = g.providers.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"UsersService"), "UsersService in {names:?}");
    assert!(names.contains(&"AppService"), "AppService in {names:?}");
    // AppController has no `#[injectable]` — must not appear.
    assert!(
        !names.contains(&"AppController"),
        "AppController leaked into providers: {names:?}"
    );
}

#[test]
fn graph_finds_dtos() {
    let root = unique_tmp_dir("graph-dtos");
    write_fixture(&root);

    let g = repl::scan(&root.join("src")).expect("scan");
    let names: Vec<&str> = g.dtos.iter().map(|d| d.name.as_str()).collect();
    assert!(
        names.contains(&"CreateUserDto"),
        "CreateUserDto in {names:?}"
    );
    assert!(
        names.contains(&"UpdateUserDto"),
        "UpdateUserDto in {names:?}"
    );
}

// ---------------------------------------------------------------------------
// dispatch (run_cli)
// ---------------------------------------------------------------------------

#[test]
fn run_cli_rejects_unknown_subcommand() {
    let root = unique_tmp_dir("dispatch-unknown");
    write_fixture(&root);

    let err = repl::run_cli(&[
        "nope".to_string(),
        "--path".to_string(),
        root.join("src").display().to_string(),
    ]);
    assert!(err.is_err(), "expected unknown-subcommand error");
}

#[test]
fn run_cli_accepts_graph_path_flag() {
    let root = unique_tmp_dir("dispatch-graph");
    write_fixture(&root);

    let g = repl::run_cli(&[
        "graph".to_string(),
        "--path".to_string(),
        root.join("src").display().to_string(),
    ])
    .expect("graph scan should succeed");
    assert!(!g.modules.is_empty(), "modules list should not be empty");
}

#[test]
fn run_cli_accepts_routes_path_flag() {
    let root = unique_tmp_dir("dispatch-routes");
    write_fixture(&root);

    let g = repl::run_cli(&[
        "routes".to_string(),
        "--path".to_string(),
        root.join("src").display().to_string(),
    ])
    .expect("routes scan should succeed");
    assert!(!g.controllers.is_empty());
}

#[test]
fn missing_path_errors_cleanly() {
    let root = unique_tmp_dir("dispatch-missing");
    // Note: do not write fixture — the path won't exist.
    let err = repl::run_cli(&[
        "graph".to_string(),
        "--path".to_string(),
        root.join("nonexistent-src").display().to_string(),
    ]);
    assert!(err.is_err(), "expected error for missing path");
    let msg = err.unwrap_err();
    assert!(
        msg.contains("does not exist"),
        "missing-path error should explain: {msg}"
    );
}

#[test]
fn parse_live_args_requires_url() {
    let err = repl::parse_live_args(&[]).expect_err("url required");
    assert!(err.contains("--url"), "{err}");
}

#[test]
fn parse_live_args_rejects_unknown_option() {
    let err = repl::parse_live_args(&[
        "--url".to_string(),
        "http://127.0.0.1:7777".to_string(),
        "--nope".to_string(),
    ])
    .expect_err("unknown option");
    assert!(err.contains("unknown option"), "{err}");
}

#[test]
fn parse_live_args_accepts_url_and_format() {
    let opts = repl::parse_live_args(&[
        "--url".to_string(),
        "http://127.0.0.1:7777".to_string(),
        "--format".to_string(),
        "json".to_string(),
    ])
    .expect("parse");
    assert_eq!(opts.url, "http://127.0.0.1:7777");
    assert_eq!(opts.format, "json");
    assert!(opts.bearer.is_none());
}
