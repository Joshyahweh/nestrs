//! Wave 7.7 — Drizzle adapter smoke test. The compile-time assertions
//! (i.e. that `DrizzleService::new()` compiles, that `DrizzleOptions::new`
//! auto-detects the right backend from the URL scheme) are the real test
//! here; runtime checks verify the detection logic.

use nestrs_drizzle::{DrizzleModule, DrizzleOptions, DrizzleService};

#[test]
fn for_root_sets_options() {
    DrizzleModule::for_root("postgres://user:pass@localhost/app");
    let svc = DrizzleService::new();
    assert_eq!(svc.url().unwrap(), "postgres://user:pass@localhost/app");
}

#[test]
fn url_scheme_detects_postgres() {
    DrizzleModule::for_root("postgres://localhost/app");
    let svc = DrizzleService::new();
    assert!(svc.is_postgres());
    assert!(!svc.is_mysql());
    assert!(!svc.is_sqlite());
}

#[test]
fn url_scheme_detects_mysql() {
    DrizzleModule::for_root("mysql://localhost/app");
    let svc = DrizzleService::new();
    assert!(svc.is_mysql());
    assert!(!svc.is_postgres());
    assert!(!svc.is_sqlite());
}

#[test]
fn url_scheme_detects_sqlite() {
    DrizzleModule::for_root("sqlite:///tmp/app.db");
    let svc = DrizzleService::new();
    assert!(svc.is_sqlite());
    assert!(!svc.is_postgres());
    assert!(!svc.is_mysql());
}

#[test]
fn drizzle_options_builder_overrides() {
    let opts = DrizzleOptions::new("postgres://localhost/app")
        .max_pool_size(8)
        .connect_timeout(std::time::Duration::from_secs(5));
    assert_eq!(opts.max_pool_size, Some(8));
    assert_eq!(
        opts.connect_timeout,
        Some(std::time::Duration::from_secs(5))
    );
}

#[test]
fn parsed_url_resolves() {
    let opts = DrizzleOptions::new("postgres://user:pass@localhost:5432/app");
    let u = opts.parsed().unwrap();
    assert_eq!(u.scheme(), "postgres");
    assert_eq!(u.host_str(), Some("localhost"));
    assert_eq!(u.port(), Some(5432));
    assert_eq!(u.path(), "/app");
}