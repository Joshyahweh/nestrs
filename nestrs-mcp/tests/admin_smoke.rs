//! Live smoke test for the `nestrs::admin` sidecar port and the
//! `nestrs-mcp` runtime client that talks to it.
//!
//! Spins up an in-process nestrs app on an ephemeral port with
//! `use_admin` enabled, hits each admin endpoint with `reqwest`, then
//! drives the same endpoints through `nestrs_mcp::runtime::AdminClient`
//! to verify the wire shape matches what the MCP runtime tools expect.

#![cfg(feature = "admin")]

use std::net::SocketAddr;

use nestrs::admin::AdminOptions;
use nestrs::prelude::*;
use nestrs::{injectable, module};
use nestrs_mcp::runtime::AdminClient;

#[injectable]
struct ProbeService;

#[module(providers = [ProbeService])]
struct ProbeModule;

#[tokio::test]
async fn admin_port_serves_health_routes_providers() {
    // Reset the global registries so this test is deterministic when
    // run alongside others in the same process.
    use nestrs_core::MetadataRegistry;
    use nestrs_core::RouteRegistry;
    RouteRegistry::clear_for_tests();
    MetadataRegistry::clear_for_tests();

    // Pre-populate the global route registry so the admin snapshot has
    // something to report. (In a real app these come from `#[routes]`
    // proc-macro expansion at build time.)
    RouteRegistry::register("GET", "/probe", "ProbeController::probe");
    RouteRegistry::register("POST", "/probe", "ProbeController::create");

    // Bind a port up-front so we know the addr the listener is on.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener); // free it; use_admin will re-bind

    let app = NestFactory::create::<ProbeModule>().enable_health_check("/live");
    let handle = app.use_admin(AdminOptions {
        addr,
        token: Some("smoke-token".into()),
    });
    tokio::spawn(async move {
        let _ = handle.serve().await;
    });

    // Give the listener a moment to bind.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    // 1) /__nestrs/health
    let health: serde_json::Value = http
        .get(format!("{base}/__nestrs/health"))
        .bearer_auth("smoke-token")
        .send()
        .await
        .expect("health request")
        .json()
        .await
        .expect("health json");
    assert_eq!(health["status"], "ok");
    assert!(health["uptime_ms"].is_u64(), "uptime_ms is u64");
    assert!(health["version"].is_string(), "version is string");

    // 2) /__nestrs/providers — should contain ProbeService
    let providers: Vec<serde_json::Value> = http
        .get(format!("{base}/__nestrs/providers"))
        .bearer_auth("smoke-token")
        .send()
        .await
        .expect("providers request")
        .json()
        .await
        .expect("providers json");
    let names: Vec<&str> = providers
        .iter()
        .filter_map(|p| p.get("type_name").and_then(|v| v.as_str()))
        .collect();
    assert!(
        names.iter().any(|n| n.contains("ProbeService")),
        "expected ProbeService in providers, got {names:?}"
    );

    // 3) /__nestrs/routes — should contain the two routes we registered
    let routes: Vec<serde_json::Value> = http
        .get(format!("{base}/__nestrs/routes"))
        .bearer_auth("smoke-token")
        .send()
        .await
        .expect("routes request")
        .json()
        .await
        .expect("routes json");
    let paths: Vec<&str> = routes
        .iter()
        .filter_map(|r| r.get("path").and_then(|v| v.as_str()))
        .collect();
    assert!(
        paths.contains(&"/probe"),
        "expected /probe in routes, got {paths:?}"
    );

    // 4) Drive the same endpoints through the MCP runtime client.
    let c = AdminClient::new(base.clone(), Some("smoke-token".into()))
        .expect("client build");
    let h = c.health().await.expect("client health");
    assert_eq!(h.status, "ok");
    let p = c.providers().await.expect("client providers");
    assert!(
        p.0.iter().any(|x| x.type_name.contains("ProbeService")),
        "expected ProbeService via AdminClient"
    );
    let r = c.routes().await.expect("client routes");
    assert!(
        r.0.iter().any(|x| x.path == "/probe"),
        "expected /probe via AdminClient"
    );

    // 5) Wrong token must 401.
    let bad = http
        .get(format!("{base}/__nestrs/health"))
        .bearer_auth("wrong")
        .send()
        .await
        .expect("bad-token request");
    assert_eq!(bad.status().as_u16(), 401);
}
