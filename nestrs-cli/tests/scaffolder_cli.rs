//! Wave 7.9 — `nestrs-cli new app|lib|resource` scaffolder integration
//! tests. Each test runs the CLI binary in a temp directory and asserts
//! the expected file layout.
//!
//! Test pattern mirrors `generator_cli.rs` — `unique_tmp_dir` for fresh
//! state per test, `cli_bin` to find the binary across toolchain
//! versions, `run_cli` to invoke with the args we want.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("nestrs-cli-{name}-{nanos}"))
}

fn cli_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_nestrs-cli") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_nestrs_cli") {
        return PathBuf::from(p);
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set for integration tests");
    let crate_dir = PathBuf::from(manifest);
    let workspace_root = crate_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("nestrs-cli").exists())
        .expect("workspace root should contain nestrs-cli/ and Cargo.toml");
    let mut debug_dir = workspace_root.join("target").join("debug");
    let mut candidates = vec![
        debug_dir.join("nestrs-cli"),
        debug_dir.join("nestrs-cli.exe"),
        debug_dir.join("deps").join("nestrs-cli"),
        debug_dir.join("deps").join("nestrs-cli.exe"),
    ];
    if !debug_dir.exists() {
        debug_dir = workspace_root.join("target").join("release");
        candidates = vec![
            debug_dir.join("nestrs-cli"),
            debug_dir.join("nestrs-cli.exe"),
            debug_dir.join("deps").join("nestrs-cli"),
            debug_dir.join("deps").join("nestrs-cli.exe"),
        ];
    }
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("could not locate nestrs-cli binary in {debug_dir:?}"))
}

fn run_cli(args: &[&str]) {
    let status = Command::new(cli_bin())
        .current_dir(std::env::temp_dir())
        .args(args)
        .status()
        .expect("failed to invoke nestrs-cli");
    assert!(status.success(), "nestrs-cli {args:?} failed");
}

fn assert_exists(path: &Path) {
    assert!(
        path.exists(),
        "expected file at {} does not exist",
        path.display()
    );
}

// ---------------------------------------------------------------------------
// `nestrs-cli new app <name>`
// ---------------------------------------------------------------------------

#[test]
fn new_app_creates_binary_crate() {
    let out = unique_tmp_dir("new-app");
    let name = out.file_name().unwrap().to_str().unwrap();
    run_cli(&["new", "app", name, "--no-git"]);
    assert_exists(&out.join("Cargo.toml"));
    assert_exists(&out.join("src").join("main.rs"));
    assert_exists(&out.join("README.md"));
    assert_exists(&out.join(".env.example"));
    assert_exists(&out.join(".gitignore"));

    let main_rs = fs::read_to_string(out.join("src/main.rs")).expect("read main.rs");
    assert!(
        main_rs.contains("NestFactory::create::<AppModule>()"),
        "main.rs must boot AppModule"
    );
    assert!(
        main_rs.contains("#[controller"),
        "main.rs must declare a controller"
    );
    assert!(
        main_rs.contains("#[dto]"),
        "main.rs must declare a DTO via #[dto]"
    );
}

// ---------------------------------------------------------------------------
// `nestrs-cli new lib <name>`
// ---------------------------------------------------------------------------

#[test]
fn new_lib_creates_library_crate() {
    let out = unique_tmp_dir("new-lib");
    let name = out.file_name().unwrap().to_str().unwrap();
    run_cli(&["new", "lib", name, "--no-git"]);
    assert_exists(&out.join("Cargo.toml"));
    assert_exists(&out.join("src").join("lib.rs"));
    assert_exists(&out.join("README.md"));
    assert_exists(&out.join(".gitignore"));

    let lib_rs = fs::read_to_string(out.join("src/lib.rs")).expect("read lib.rs");
    assert!(
        lib_rs.contains("pub mod controllers"),
        "lib.rs must declare a controllers module"
    );
    assert!(
        lib_rs.contains("pub mod services"),
        "lib.rs must declare a services module"
    );
    assert!(
        lib_rs.contains("pub mod dto"),
        "lib.rs must declare a dto module"
    );
}

// ---------------------------------------------------------------------------
// `nestrs-cli new resource <name>`
// ---------------------------------------------------------------------------

#[test]
fn new_resource_emits_controller_service_dto_module() {
    let out = unique_tmp_dir("new-resource");
    let name = out.file_name().unwrap().to_str().unwrap();
    run_cli(&["new", "resource", name, "--no-git"]);
    let resource_dir = out.join("src").join(name);
    assert_exists(&resource_dir.join("dto.rs"));
    assert_exists(&resource_dir.join("service.rs"));
    assert_exists(&resource_dir.join("controller.rs"));
    assert_exists(&resource_dir.join("module.rs"));
    assert_exists(&resource_dir.join("mod.rs"));

    let dto_rs = fs::read_to_string(resource_dir.join("dto.rs")).expect("read dto.rs");
    assert!(
        dto_rs.contains("#[dto]"),
        "dto.rs must use #[dto] for create DTO"
    );
    assert!(
        dto_rs.contains("#[nestrs::partial_type]"),
        "dto.rs must use Wave 7.5 partial_type for update DTO"
    );

    let controller_rs =
        fs::read_to_string(resource_dir.join("controller.rs")).expect("read controller.rs");
    assert!(
        controller_rs.contains("#[controller"),
        "controller.rs must declare a controller"
    );
    assert!(
        controller_rs.contains("ValidatedBody"),
        "controller.rs must use ValidatedBody extractor"
    );

    let module_rs = fs::read_to_string(resource_dir.join("module.rs")).expect("read module.rs");
    assert!(
        module_rs.contains("#[module("),
        "module.rs must declare a #[module(...)]"
    );
    assert!(
        module_rs.contains("controllers = [")
            && module_rs.contains("providers = [")
            && module_rs.contains("exports = ["),
        "module.rs must list controllers / providers / exports"
    );

    let service_rs = fs::read_to_string(resource_dir.join("service.rs")).expect("read service.rs");
    assert!(
        service_rs.contains("#[injectable]"),
        "service.rs must use #[injectable]"
    );
}

// ---------------------------------------------------------------------------
// Back-compat: `nestrs-cli new <name>` (no `app`/`lib`/`resource` keyword)
// still creates an app, matching the original 1.0.0 signature.
// ---------------------------------------------------------------------------

#[test]
fn new_without_kind_keyword_still_creates_app() {
    let out = unique_tmp_dir("new-bare");
    let name = out.file_name().unwrap().to_str().unwrap();
    run_cli(&["new", name, "--no-git"]);
    assert_exists(&out.join("src").join("main.rs"));
}
