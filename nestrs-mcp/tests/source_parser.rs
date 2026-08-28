//! Source parser unit tests. Spins up tiny temporary workspaces and runs
//! `introspection::parse_workspace` over them, asserting that modules,
//! controllers, providers, and DTOs come back with the right shape.

use std::fs;
use std::path::Path;

use nestrs_mcp::introspection::parse_workspace;

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

#[test]
fn parses_module_controller_provider_dto() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cargo_toml(root, "fixture");

    write_source(
        root,
        "src/lib.rs",
        r#"
use nestrs::{controller, dto, injectable, module, routes, Module};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Default, Module)]
#[module(controllers = [UserController], providers = [UserService], exports = [UserService])]
pub struct AppModule;

#[derive(Default)]
#[injectable]
pub struct UserService {
    inner: String,
}

#[derive(Default)]
#[controller("/users")]
pub struct UserController;

#[routes(UserController)]
impl UserController {
    #[get("/")]
    async fn list(&self) -> Vec<String> {
        vec![]
    }
}

#[derive(Debug, Serialize, Deserialize, Validate)]
#[dto]
pub struct UserDto {
    #[validate(length(min = 1))]
    pub name: String,
}
"#,
    );

    let parsed = parse_workspace(root).expect("parse should succeed");
    assert!(
        parsed.modules.iter().any(|m| m.name == "AppModule"),
        "expected to find AppModule, got {:?}",
        parsed.modules.iter().map(|m| &m.name).collect::<Vec<_>>()
    );
    assert!(
        parsed
            .controllers
            .iter()
            .any(|c| c.name == "UserController"),
        "expected to find UserController"
    );
    assert!(
        parsed
            .providers
            .iter()
            .any(|p| p.type_name == "UserService"),
        "expected to find UserService"
    );
    assert!(
        parsed.dtos.iter().any(|d| d.name == "UserDto"),
        "expected to find UserDto"
    );
}

#[test]
fn unknown_attrs_do_not_fail_parse() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_cargo_toml(root, "fixture2");

    write_source(
        root,
        "src/lib.rs",
        r#"
use nestrs::{module, Module};

#[derive(Default, Module)]
#[module(some_future_attr = "ignored")]
pub struct AppModule;
"#,
    );

    let parsed = parse_workspace(root).expect("parse should not fail");
    assert_eq!(parsed.modules.len(), 1);
    // The unknown attr is parsed as a warning or silently dropped, not a hard error.
}
