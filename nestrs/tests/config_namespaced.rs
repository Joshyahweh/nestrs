//! Wave 3E.4 — namespaced config integration tests (`NESTRS_<NS>__<KEY>`).
//!
//! Covers the `#[config(namespace = ...)]` macro, `ConfigModule::for_root`,
//! `ConfigService::get`, and the real-process-env path (`#[serial]`).

use nestrs::{config, Config, ConfigModule, ConfigNamespace, ConfigService};
use serial_test::serial;

#[config(namespace = "db")]
#[derive(Debug)]
struct DatabaseConfig {
    #[validate(length(min = 1))]
    host: String,
    #[serde(default)]
    port: u16,
}

#[config(namespace = "auth")]
#[derive(Debug)]
struct AuthConfig {
    #[validate(length(min = 1))]
    name: String,
}

fn overlay(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn config_macro_sets_namespace_and_derives() {
    // The macro emitted Deserialize + Validate + ConfigNamespace — this
    // function is the compile-time proof via trait bounds; the const is the
    // runtime one.
    assert_eq!(<DatabaseConfig as ConfigNamespace>::NAMESPACE, "db");
    assert_eq!(<AuthConfig as ConfigNamespace>::NAMESPACE, "auth");
    fn assert_traits<T: serde::de::DeserializeOwned + validator::Validate>() {}
    assert_traits::<DatabaseConfig>();
    assert_traits::<AuthConfig>();
}

#[test]
fn single_namespace_round_trip_through_config_service() {
    let o = overlay(&[
        ("NESTRS_DB__HOST", "db.internal"),
        ("NESTRS_DB__PORT", "5433"),
    ]);
    let svc =
        ConfigService::build_with_prefix(&[Config::register::<DatabaseConfig>()], &o, "NESTRS_")
            .expect("build");
    let db = svc.get::<DatabaseConfig>().expect("typed get");
    assert_eq!(db.host, "db.internal");
    assert_eq!(db.port, 5433);
}

#[test]
fn multiple_namespaces_with_overlapping_keys() {
    let o = overlay(&[
        ("NESTRS_DB__HOST", "postgres"),
        ("NESTRS_AUTH__NAME", "authsvc"),
    ]);
    let svc = ConfigService::build_with_prefix(
        &[
            Config::register::<DatabaseConfig>(),
            Config::register::<AuthConfig>(),
        ],
        &o,
        "NESTRS_",
    )
    .expect("build");
    assert_eq!(svc.get::<DatabaseConfig>().expect("db").host, "postgres");
    assert_eq!(svc.get::<AuthConfig>().expect("auth").name, "authsvc");
}

#[test]
fn validator_rejects_invalid_env_at_boot() {
    let o = overlay(&[("NESTRS_DB__HOST", "")]);
    let err =
        ConfigService::build_with_prefix(&[Config::register::<DatabaseConfig>()], &o, "NESTRS_")
            .expect_err("empty host must fail validation");
    assert!(err.message.contains("db"), "{err}");
}

#[test]
fn env_prefix_override_is_honored() {
    let o = overlay(&[("APP_DB__HOST", "prefixed")]);
    let svc = ConfigService::build_with_prefix(&[Config::register::<DatabaseConfig>()], &o, "APP_")
        .expect("build");
    assert_eq!(svc.get::<DatabaseConfig>().expect("db").host, "prefixed");
}

#[test]
#[serial]
fn real_process_env_drives_config_module_for_root() {
    std::env::set_var("NESTRS_DB__HOST", "from-process-env");
    std::env::set_var("NESTRS_DB__PORT", "7788");

    let module = ConfigModule::for_root(vec![Config::register::<DatabaseConfig>()]);
    let svc = module.registry.get::<ConfigService>();
    let db = svc.get::<DatabaseConfig>().expect("typed get");
    assert_eq!(db.host, "from-process-env");
    assert_eq!(db.port, 7788);

    std::env::remove_var("NESTRS_DB__HOST");
    std::env::remove_var("NESTRS_DB__PORT");
}
