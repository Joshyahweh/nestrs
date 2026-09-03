//! Spawn the `nestrs-mcp` binary in HTTP mode as a detached background
//! process, then health-check it via reqwest.
//!
//! The `tokio::process::Child` is returned alongside the metadata so the
//! caller can keep it alive (and the OS process stays up) for as long as
//! the wizard process is running.

use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tokio::process::{Child, Command};

/// Result of a successful background spawn.
#[derive(Debug)]
pub struct SpawnedServer {
    /// OS PID of the spawned process.
    pub pid: u32,
    /// Resolved bind address (`127.0.0.1:7777` by default).
    pub addr: SocketAddr,
    /// Full URL the client should hit: `http://<addr>/mcp`.
    pub url: String,
    /// Live handle to the child. Kept in the struct so the process is not
    /// dropped (and therefore not killed) when this struct is returned.
    pub child: Child,
}

/// Spawn `nestrs-mcp --transport http --http-addr <addr>` and wait for it
/// to respond to an `initialize` request.
pub async fn spawn_http_server(bin: &Path, addr: &str) -> Result<SpawnedServer> {
    let socket_addr: SocketAddr = addr
        .parse()
        .with_context(|| format!("invalid --http-addr `{addr}`"))?;
    let url = format!("http://{addr}/mcp");

    // Spawn detached: stdout → /dev/null, stderr inherited so the user
    // sees any server-side error on their terminal.
    let mut child = Command::new(bin)
        .args(["--transport", "http", "--http-addr", addr])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .stdin(Stdio::null())
        .kill_on_drop(false)
        .spawn()
        .with_context(|| format!("failed to spawn {} --transport http", bin.display()))?;

    let pid = child
        .id()
        .ok_or_else(|| anyhow!("spawned child has no pid; OS may not support it"))?;

    // Health check: up to 20 attempts × 200ms = 4s total.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .context("failed to build reqwest client")?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "nestrs-mcp-init", "version": env!("CARGO_PKG_VERSION")},
        }
    });

    let mut last_err: Option<String> = None;
    for attempt in 0..20 {
        if let Some(status) = child.try_wait().context("try_wait on spawned child")? {
            // Process exited on its own — surface the failure.
            return Err(anyhow!(
                "spawned nestrs-mcp exited with {status} during health check. \
                 Try a different --http-addr (e.g. --http-addr 127.0.0.1:7778)."
            ));
        }
        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                println!("Health check OK (initialize response {}).", resp.status());
                return Ok(SpawnedServer {
                    pid,
                    addr: socket_addr,
                    url,
                    child,
                });
            }
            Ok(resp) => {
                last_err = Some(format!("HTTP {}", resp.status()));
            }
            Err(e) => {
                last_err = Some(e.to_string());
            }
        }
        // Don't sleep after the last attempt.
        if attempt < 19 {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    // All attempts failed — kill the child and surface the last error.
    let _ = child.kill().await;
    Err(anyhow!(
        "spawned server did not respond at {url} within 4s (last error: {}). \
         Check `nestrs-mcp --transport http --http-addr {addr}` manually.",
        last_err.unwrap_or_else(|| "unknown".to_string())
    ))
}
