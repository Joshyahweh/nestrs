//! Post-install setup wizard for `nestrs-mcp`.
//!
//! `nestrs-mcp init` (alias: `setup`) detects installed editors by checking
//! the well-known config file paths, asks the user which ones to configure
//! via a hand-rolled multi-select checklist, and writes the right MCP server
//! entry into each one (idempotently preserving everything else). When
//! configured for HTTP transport, it can also spawn the server in the
//! background and print its URL.
//!
//! Mirrors the hand-rolled prompt style of `nestrs-cli/src/main.rs` (no
//! `dialoguer` / `inquire` / `indicatif` dependencies).

pub mod editors;
pub mod json_merge;
pub mod prompt;
pub mod spawn;
pub mod summary;
pub mod toml_merge;

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

use editors::Editor;

/// CLI args for the wizard. Mirrors the clap `InitArgs` struct in `main.rs`.
#[derive(Debug, Clone)]
pub struct InitArgs {
    pub yes: bool,
    pub no_interactive: bool,
    pub transport: WizardTransport,
    pub http_addr: Option<String>,
    pub start_http_server: bool,
}

/// Transport the wizard configures. Mirrors the binary's `Transport` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardTransport {
    Stdio,
    Http,
}

impl WizardTransport {
    /// Build the right JSON value for the nestrs entry, given the URL the
    /// HTTP server will be at (only used when `self == Http`).
    pub fn server_value(self, http_url: &str) -> ServerValue {
        match self {
            WizardTransport::Stdio => ServerValue::Json(serde_json::json!({
                "command": "nestrs-mcp",
                "args": []
            })),
            WizardTransport::Http => ServerValue::Json(serde_json::json!({
                "url": http_url
            })),
        }
    }
}

/// Either a JSON object (for Claude Code / Cursor / VS Code) or a TOML
/// inline table (for Codex). The merge functions know how to write each.
#[derive(Debug, Clone)]
pub enum ServerValue {
    Json(JsonValue),
    Toml(TomlValue),
}

/// Outcome of a wizard run. Returned from `run` so tests and `--no-interactive`
/// can assert on what would have happened.
#[derive(Debug)]
pub struct WizardOutcome {
    pub selected: Vec<Editor>,
    pub written: Vec<WriteResult>,
    pub transport: WizardTransport,
    pub server_pid: Option<u32>,
    pub server_url: Option<String>,
    /// Kept alive for the lifetime of the wizard. Not Debug/Clone because
    /// `tokio::process::Child` isn't.
    pub _server: Option<spawn::SpawnedServer>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct WriteResult {
    pub editor: Editor,
    pub path: PathBuf,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Created,
    Added,
    Updated,
    NoChange,
}

/// Default HTTP address used when the user doesn't pass `--http-addr`.
pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:7777";

/// Top-level entry point for the wizard. The binary's `main()` calls this
/// when `init` or `setup` is invoked.
pub fn run(args: InitArgs) -> Result<WizardOutcome> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    let is_tty = std::io::stdin().is_terminal();

    // 1. Detect which editors have a config file on disk.
    let detected = editors::detect(&cwd);
    let all = Editor::all();

    // 2. Ask the user which ones to configure (or use the flag).
    let selected = prompt::select_editors(&detected, &all, &args, is_tty)?;

    // 3. Ask the user which transport (or use the flag).
    let transport = prompt::select_transport(&args, is_tty)?;

    // 4. Resolve the HTTP URL if we're going to spawn or write an HTTP entry.
    let http_addr = args
        .http_addr
        .as_deref()
        .unwrap_or(DEFAULT_HTTP_ADDR)
        .to_string();
    let http_url = format!("http://{http_addr}/mcp");

    // 5. Build the per-editor server value, with the right extra fields.
    let mut written = Vec::with_capacity(selected.len());
    let mut toml_files_touched = false;
    for &editor in &selected {
        let path = editor
            .path(&cwd)
            .with_context(|| format!("failed to resolve config path for {editor:?}"))?;
        let value = server_value_for(editor, transport, &http_url);
        let outcome = if args.no_interactive {
            // --no-interactive is the dry-run mode: record what would have
            // been written but don't touch the filesystem.
            println!(
                "[no-interactive] would write {} ({})",
                path.display(),
                format_kind(editor)
            );
            WriteOutcome::NoChange
        } else {
            let outcome = match &value {
                ServerValue::Json(v) => {
                    let top = editor.top_level_key();
                    let r = json_merge::merge_json(&path, top, "nestrs", v.clone())?;
                    println!("Wrote {} ({})", path.display(), format_kind(editor));
                    r
                }
                ServerValue::Toml(v) => {
                    toml_files_touched = true;
                    let r = toml_merge::merge_toml(&path, "mcp_servers", "nestrs", v.clone())?;
                    println!("Wrote {} ({})", path.display(), format_kind(editor));
                    r
                }
            };
            outcome
        };
        written.push(WriteResult {
            editor,
            path,
            outcome,
        });
    }

    // 6. Optionally spawn the HTTP server in the background.
    let mut server = None;
    if !args.no_interactive && args.start_http_server {
        match transport {
            WizardTransport::Http => {
                // We're sync but `spawn` is async. We use `tokio::runtime::Handle::current()`
                // so the wizard still works inside the `#[tokio::main]` binary.
                let handle = tokio::runtime::Handle::try_current().context(
                    "--start-http-server requires a tokio runtime; the binary always provides one",
                )?;
                let bin = std::env::current_exe().context("failed to resolve current_exe")?;
                server = Some(handle.block_on(spawn::spawn_http_server(&bin, &http_addr))?);
            }
            WizardTransport::Stdio => {
                eprintln!("--start-http-server has no effect with --transport=stdio; ignoring.");
            }
        }
    }

    // 7. Print the closing banner.
    let outcome = WizardOutcome {
        selected,
        written,
        transport,
        server_pid: server.as_ref().map(|s| s.pid),
        server_url: server.as_ref().map(|s| s.url.clone()),
        _server: server,
        dry_run: args.no_interactive,
    };
    summary::print_summary(&outcome, toml_files_touched);

    Ok(outcome)
}

fn server_value_for(editor: Editor, transport: WizardTransport, http_url: &str) -> ServerValue {
    use editors::Editor::*;
    match (editor, transport) {
        (VsCodeCopilot, WizardTransport::Stdio) => ServerValue::Json(serde_json::json!({
            "type": "stdio",
            "command": "nestrs-mcp",
            "args": []
        })),
        (VsCodeCopilot, WizardTransport::Http) => ServerValue::Json(serde_json::json!({
            "type": "http",
            "url": http_url
        })),
        // Codex uses TOML with a different field set.
        (Codex, WizardTransport::Stdio) => ServerValue::Toml(
            toml::toml! {
                command = "nestrs-mcp"
                args = []
            }
            .into(),
        ),
        (Codex, WizardTransport::Http) => ServerValue::Toml(
            toml::toml! {
                url = http_url
            }
            .into(),
        ),
        // Claude Code + Cursor (project + global): same shape.
        (_, WizardTransport::Stdio) => ServerValue::Json(serde_json::json!({
            "command": "nestrs-mcp",
            "args": []
        })),
        (_, WizardTransport::Http) => ServerValue::Json(serde_json::json!({
            "url": http_url
        })),
    }
}

fn format_kind(editor: Editor) -> &'static str {
    if editor.is_toml() {
        "toml"
    } else {
        "json"
    }
}

use std::io::IsTerminal;
