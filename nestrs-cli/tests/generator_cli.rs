use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("nestrs-cli-{name}-{nanos}"))
}

fn run_cli(args: &[&str]) {
    let bin = env!("CARGO_BIN_EXE_nestrs");
    let status = Command::new(bin)
        .args(args)
        .status()
        .expect("failed to execute nestrs cli");
    assert!(status.success(), "cli exited with non-zero status");
}

fn run_cli_output(args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_nestrs");
    Command::new(bin)
        .args(args)
        .output()
        .expect("failed to execute nestrs cli with output capture")
}

fn assert_exists(path: &Path) {
    assert!(path.exists(), "expected path to exist: {}", path.display());
}

#[test]
fn generate_service_nest_style() {
    let out = unique_tmp_dir("service-nest");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "service",
        "learner-stats",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let path = out.join("learner_stats.service.rs");
    assert_exists(&path);
    let src = fs::read_to_string(&path).expect("read service file");
    assert!(
        src.contains("#[derive(Default)]"),
        "service needs Default for #[injectable]"
    );
    assert!(
        src.contains("#[injectable]"),
        "service should use #[injectable]"
    );
}

#[test]
fn generate_resource_rest_nest_style() {
    let out = unique_tmp_dir("resource-rest");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "resource",
        "learner-stats",
        "--transport",
        "rest",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let feature = out.join("learner_stats");
    assert_exists(&feature.join("learner_stats.module.rs"));
    assert_exists(&feature.join("learner_stats.controller.rs"));
    assert_exists(&feature.join("learner_stats.service.rs"));
    assert_exists(&feature.join("create_learner_stats.dto.rs"));
    assert_exists(&feature.join("update_learner_stats.dto.rs"));
    assert_exists(&feature.join("mod.rs"));

    let mod_rs = fs::read_to_string(feature.join("mod.rs")).expect("read mod.rs");
    assert!(
        mod_rs.contains("#[path = \"learner_stats.controller.rs\"]"),
        "expected #[path] mod mapping for dotted filename"
    );

    let controller =
        fs::read_to_string(feature.join("learner_stats.controller.rs")).expect("read controller");
    assert!(
        controller.contains("#[post(\"/\")]") && controller.contains("#[put(\"/:id\")]"),
        "expected full REST CRUD route handlers in generated controller"
    );
    let service =
        fs::read_to_string(feature.join("learner_stats.service.rs")).expect("read service");
    assert!(
        service.contains("pub async fn list") && service.contains("pub async fn delete"),
        "expected CRUD methods on generated service"
    );
}

#[test]
fn generate_resource_ws_rust_style() {
    let out = unique_tmp_dir("resource-ws-rust");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "resource",
        "chat-room",
        "--transport",
        "ws",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let feature = out.join("chat_room");
    assert_exists(&feature.join("chat_room_module.rs"));
    assert_exists(&feature.join("chat_room_gateway.rs"));
    assert_exists(&feature.join("chat_room_service.rs"));
    assert_exists(&feature.join("create_chat_room_dto.rs"));
    assert_exists(&feature.join("update_chat_room_dto.rs"));
    assert_exists(&feature.join("mod.rs"));

    let gateway = fs::read_to_string(feature.join("chat_room_gateway.rs")).expect("read gateway");
    assert!(
        gateway.contains("#[subscribe_message(") && gateway.contains(".list"),
        "expected WebSocket subscribe_message handlers for CRUD-style events"
    );
}

#[test]
fn generate_dto_nest_style() {
    let out = unique_tmp_dir("dto-nest");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "dto",
        "invite-user",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let dto_file = out.join("invite_user.dto.rs");
    assert_exists(&dto_file);
    let content = fs::read_to_string(dto_file).expect("read dto file");
    assert!(
        content.contains("#[dto]"),
        "dto template should include #[dto]"
    );
}

#[test]
fn generate_cross_cutting_files_rust_style() {
    let out = unique_tmp_dir("cross-cutting-rust");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "guard",
        "auth",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "pipe",
        "validation",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "filter",
        "http_exception",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "interceptor",
        "logging",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "strategy",
        "jwt",
        "--style",
        "rust",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let auth_guard = out.join("auth_guard.rs");
    assert_exists(&auth_guard);
    let guard_src = fs::read_to_string(&auth_guard).expect("read auth_guard.rs");
    assert!(
        guard_src.contains("impl CanActivate"),
        "guard template should implement CanActivate for impl_routes!"
    );
    assert_exists(&out.join("validation_pipe.rs"));
    assert_exists(&out.join("http_exception_filter.rs"));
    assert_exists(&out.join("logging_interceptor.rs"));
    assert_exists(&out.join("jwt_strategy.rs"));
}

#[test]
fn generate_phase4_like_files_nest_style() {
    let out = unique_tmp_dir("phase4-nest");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "resolver",
        "users",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "gateway",
        "chat",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "microservice",
        "billing",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);
    run_cli(&[
        "g",
        "transport",
        "nats",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    assert_exists(&out.join("users.resolver.rs"));
    assert_exists(&out.join("chat.gateway.rs"));
    assert_exists(&out.join("billing.microservice.rs"));
    assert_exists(&out.join("nats.transport.rs"));
}

#[test]
fn resource_no_interactive_defaults_to_rest() {
    let out = unique_tmp_dir("resource-no-interactive");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "resource",
        "orders",
        "--no-interactive",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let feature = out.join("orders");
    assert_exists(&feature.join("orders.controller.rs"));
}

#[test]
fn dry_run_does_not_create_files() {
    let out = unique_tmp_dir("dry-run");
    fs::create_dir_all(&out).expect("create temp output dir");

    run_cli(&[
        "g",
        "service",
        "billing",
        "--dry-run",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let generated = out.join("billing.service.rs");
    assert!(
        !generated.exists(),
        "expected dry-run not to create file: {}",
        generated.display()
    );
}

#[test]
fn force_overwrites_existing_file() {
    let out = unique_tmp_dir("force-overwrite");
    fs::create_dir_all(&out).expect("create temp output dir");

    let existing = out.join("billing.service.rs");
    fs::write(&existing, "old-content").expect("seed existing file");

    let bin = env!("CARGO_BIN_EXE_nestrs");
    let status_without_force = Command::new(bin)
        .args([
            "g",
            "service",
            "billing",
            "--style",
            "nest",
            "--path",
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .expect("run cli without force");
    assert!(
        !status_without_force.success(),
        "expected failure when file exists without --force"
    );

    run_cli(&[
        "g",
        "service",
        "billing",
        "--style",
        "nest",
        "--force",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let new_content = fs::read_to_string(existing).expect("read overwritten file");
    assert!(
        new_content.contains("pub struct BillingService"),
        "expected generated content after --force overwrite"
    );
}

#[test]
fn resource_wires_parent_lib_rs() {
    let out = unique_tmp_dir("wire-parent");
    fs::create_dir_all(&out).expect("create temp output dir");
    let lib_rs = out.join("lib.rs");
    fs::write(&lib_rs, "// root\n").expect("seed lib.rs");

    run_cli(&[
        "g",
        "resource",
        "billing",
        "--transport",
        "rest",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let content = fs::read_to_string(lib_rs).expect("read updated lib.rs");
    assert!(
        content.contains("pub mod billing;"),
        "expected module insertion"
    );
    assert!(
        content.contains("use crate::billing::BillingModule;"),
        "expected use insertion"
    );
}

#[test]
fn resource_wiring_dry_run_does_not_change_parent() {
    let out = unique_tmp_dir("wire-parent-dry");
    fs::create_dir_all(&out).expect("create temp output dir");
    let lib_rs = out.join("lib.rs");
    fs::write(&lib_rs, "// root\n").expect("seed lib.rs");

    run_cli(&[
        "g",
        "resource",
        "catalog",
        "--dry-run",
        "--no-interactive",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    let content = fs::read_to_string(lib_rs).expect("read lib.rs after dry-run");
    assert_eq!(content, "// root\n", "dry-run should not modify lib.rs");
}

#[test]
fn quiet_suppresses_non_error_output() {
    let out = unique_tmp_dir("quiet");
    fs::create_dir_all(&out).expect("create temp output dir");
    let output = run_cli_output(&[
        "g",
        "service",
        "audit",
        "--quiet",
        "--style",
        "nest",
        "--path",
        out.to_str().expect("utf8 path"),
    ]);

    assert!(output.status.success(), "quiet command should succeed");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(
        stdout.trim().is_empty(),
        "quiet mode should suppress stdout"
    );
    assert_exists(&out.join("audit.service.rs"));
}

#[test]
fn new_project_scaffolds_files() {
    let parent = unique_tmp_dir("new-project-parent");
    fs::create_dir_all(&parent).expect("create temp parent");
    let project_dir = parent.join("demo_app");

    let output = run_cli_output(&["new", project_dir.to_str().expect("utf8 path"), "--no-git"]);

    assert!(output.status.success(), "new command should succeed");
    assert_exists(&project_dir.join("Cargo.toml"));
    assert_exists(&project_dir.join("src/main.rs"));
    assert_exists(&project_dir.join(".env.example"));
    assert_exists(&project_dir.join(".gitignore"));
}

#[test]
fn new_project_strict_adds_serde_deny_unknown_fields() {
    let parent = unique_tmp_dir("new-project-strict");
    fs::create_dir_all(&parent).expect("create temp parent");
    let project_dir = parent.join("strict_app");

    let output = run_cli_output(&[
        "new",
        project_dir.to_str().expect("utf8 path"),
        "--no-git",
        "--strict",
    ]);

    assert!(output.status.success(), "strict new command should succeed");
    let main_rs = fs::read_to_string(project_dir.join("src/main.rs")).expect("read main");
    assert!(
        main_rs.contains("#[serde(deny_unknown_fields)]"),
        "strict scaffold should include serde deny_unknown_fields"
    );
}
