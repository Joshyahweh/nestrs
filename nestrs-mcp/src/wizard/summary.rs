//! End-of-run banner for the wizard.

use crate::wizard::{WizardOutcome, WizardTransport};

/// Print the closing summary. Idempotent (just stdout writes).
pub fn print_summary(outcome: &WizardOutcome, toml_files_touched: bool) {
    println!();
    if outcome.dry_run {
        println!("[no-interactive] dry run complete. Re-run without --no-interactive to apply.");
        return;
    }

    if outcome.written.is_empty() {
        println!("Nothing to do. Run without --no-interactive to pick editors manually.");
        return;
    }

    if let (Some(pid), Some(url)) = (outcome.server_pid, &outcome.server_url) {
        println!();
        println!("Started nestrs-mcp in the background at {url} (PID: {pid}).");
        println!("Use `kill {pid}` to stop it.");
    }

    println!();
    match outcome.transport {
        WizardTransport::Stdio => {
            println!(
                "Your editors are now configured to spawn `nestrs-mcp` over stdio. \
                 No process to manage — the editor launches the server on demand."
            );
        }
        WizardTransport::Http => {
            if outcome.server_pid.is_none() {
                println!(
                    "Configs were written for HTTP transport. \
                     Start the server with: nestrs-mcp --transport http --http-addr 127.0.0.1:7777"
                );
            } else {
                println!(
                    "Restart your editor (or click Refresh in the MCP servers panel). \
                     Server is at {}.",
                    outcome.server_url.as_deref().unwrap_or("")
                );
            }
        }
    }

    if toml_files_touched {
        println!();
        println!(
            "Note: Codex's config.toml may show unrelated diff hunks (TOML re-serialization). \
             This is a single-time diff from re-formatting; commit the new file once and you're set."
        );
    }
}
