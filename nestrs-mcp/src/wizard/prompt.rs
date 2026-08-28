//! Hand-rolled multi-select editor picker and transport picker.
//!
//! Mirrors the `IsTerminal` + `read_line` style used in `nestrs-cli/src/main.rs`.
//! No `dialoguer` / `inquire` / `indicatif` dependencies.

use std::io::{self, IsTerminal, Write};

use anyhow::{anyhow, Result};

use super::editors::Editor;
use super::{InitArgs, WizardTransport};

/// Pick which editors to configure. Returns the list in the order the user
/// toggled them (default order: top to bottom of the checklist).
///
/// Behavior:
/// - `args.yes` set → return every detected editor (no prompts).
/// - `args.no_interactive` (without `--yes`) with multiple detected → error
///   (CI scripts can branch on the exit code in `wizard::run`).
/// - Otherwise render the checklist and toggle one-by-one.
pub fn select_editors(
    detected: &[Editor],
    all: &[Editor],
    args: &InitArgs,
    is_tty: bool,
) -> Result<Vec<Editor>> {
    if args.yes {
        if !detected.is_empty() {
            return Ok(detected.to_vec());
        }
        // `--yes` with nothing detected. If we're interactive, fall through
        // to the manual checklist so the user can still pick one. If we're
        // not, return empty and let `run` print the "Nothing to do" message.
        if !is_tty {
            return Ok(Vec::new());
        }
    }

    if args.no_interactive && !args.yes && detected.len() > 1 {
        return Err(anyhow!(
            "multiple editors detected ({}). Pass --yes to accept all, or run interactively.",
            detected.len()
        ));
    }

    if !is_tty {
        // No TTY and no `--yes` flag → nothing to do, the caller will print
        // a hint. (Avoids blocking on a stdin read that would never come.)
        return Ok(detected.to_vec());
    }

    // Build the checklist. Pre-select: detected editors are on, the rest off.
    let mut selected: Vec<bool> = all.iter().map(|e| detected.contains(e)).collect();

    // Print header + the current checklist.
    println!();
    println!("Editors to configure (toggle y/n, enter to confirm):");
    let _ = io::stdout().flush();
    for (i, e) in all.iter().enumerate() {
        let mark = if selected[i] { "[y]" } else { "[n]" };
        println!("  {mark} {:<24} {}", e.label(), e.path_cwd_relative());
    }

    // Walk through each unchecked editor, asking the user to confirm or toggle.
    // We don't re-ask on already-detected editors (we assume yes for them).
    for (i, e) in all.iter().enumerate() {
        if selected[i] {
            continue;
        }
        let ans = prompt_line(&format!("Toggle [{}] (y/n, enter to skip): ", e.label()))?;
        match ans.as_str() {
            "y" | "Y" | "yes" => selected[i] = true,
            "n" | "N" | "no" | "" => {}
            other => {
                eprintln!("unrecognized answer `{other}` — leaving as [n]");
            }
        }
    }

    // After all toggles, require an explicit enter to confirm the selection.
    println!();
    let mut shown = false;
    loop {
        if !shown {
            print!("Press enter to confirm, or `cancel` to abort: ");
            shown = true;
        } else {
            print!("Press enter to confirm, or `cancel` to abort: ")
        };
        let _ = io::stdout().flush();
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(io_to_anyhow)?;
        let trimmed = input.trim();
        match trimmed {
            "" => break,
            "cancel" | "abort" | "q" | "quit" => {
                return Err(anyhow!("aborted by user"));
            }
            _ => {
                eprintln!("unrecognized answer `{trimmed}` — press enter to confirm or `cancel` to abort");
            }
        }
    }

    Ok(all
        .iter()
        .zip(selected.iter())
        .filter_map(|(e, &on)| if on { Some(*e) } else { None })
        .collect())
}

impl Editor {
    /// Path-as-string for the multi-select prompt. Project-relative paths
    /// stay relative (so the prompt is short); global ones are absolute.
    fn path_cwd_relative(self) -> String {
        let cwd = std::env::current_dir().ok();
        match (self, cwd) {
            (Editor::ClaudeCodeProject, _) => ".mcp.json".to_string(),
            (Editor::VsCodeCopilot, _) => ".vscode/mcp.json".to_string(),
            (_, Some(cwd)) => self
                .path(&cwd)
                .ok()
                .and_then(|p| p.strip_prefix(&cwd).ok().map(|p| p.to_path_buf()))
                .map(|p| format!("~/{p}", p = p.display()))
                .unwrap_or_else(|| {
                    self.path(&cwd)
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                }),
            (_, None) => self
                .path(std::path::Path::new("."))
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        }
    }
}

/// Pick which transport to configure.
pub fn select_transport(args: &InitArgs, is_tty: bool) -> Result<WizardTransport> {
    if args.no_interactive {
        return Ok(args.transport);
    }
    if !is_tty {
        // No TTY → use the flag's default.
        return Ok(args.transport);
    }
    loop {
        print!("Transport? [stdio/http] (default {}): ", transport_label(args.transport));
        let _ = io::stdout().flush();
        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(io_to_anyhow)?;
        let trimmed = input.trim().to_lowercase();
        match trimmed.as_str() {
            "" => return Ok(args.transport),
            "stdio" | "s" | "std" => return Ok(WizardTransport::Stdio),
            "http" | "h" | "https" => return Ok(WizardTransport::Http),
            other => {
                eprintln!("unrecognized transport `{other}` — enter `stdio` or `http`");
            }
        }
    }
}

fn transport_label(t: WizardTransport) -> &'static str {
    match t {
        WizardTransport::Stdio => "stdio",
        WizardTransport::Http => "http",
    }
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    let _ = io::stdout().flush();
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(io_to_anyhow)?;
    Ok(input.trim().to_lowercase())
}

fn io_to_anyhow(e: io::Error) -> anyhow::Error {
    anyhow::Error::new(e)
}

#[allow(dead_code)]
fn check_tty() -> bool {
    io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wizard::InitArgs;

    fn args(yes: bool, no_interactive: bool) -> InitArgs {
        InitArgs {
            yes,
            no_interactive,
            transport: WizardTransport::Stdio,
            http_addr: None,
            start_http_server: false,
        }
    }

    #[test]
    fn yes_returns_all_detected() {
        let detected = vec![Editor::Cursor, Editor::Codex];
        let all = Editor::all();
        let result = select_editors(&detected, &all, &args(true, false), false).unwrap();
        assert_eq!(result, detected);
    }

    #[test]
    fn no_interactive_with_multiple_errors() {
        let detected = vec![Editor::Cursor, Editor::Codex, Editor::VsCodeCopilot];
        let all = Editor::all();
        let err = select_editors(&detected, &all, &args(false, true), false).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("multiple editors"), "got: {msg}");
    }

    #[test]
    fn no_interactive_with_single_does_not_error() {
        // Single detected editor: wizard auto-picks it without prompting.
        let detected = vec![Editor::Cursor];
        let all = Editor::all();
        let result = select_editors(&detected, &all, &args(false, true), false).unwrap();
        assert_eq!(result, detected);
    }

    #[test]
    fn no_yes_no_tty_returns_detected() {
        // Non-interactive, no flag: still return what was detected (the
        // caller prints the "Nothing to do" hint if this is empty).
        let detected = vec![Editor::Cursor];
        let all = Editor::all();
        let result = select_editors(&detected, &all, &args(false, false), false).unwrap();
        assert_eq!(result, detected);
    }

    #[test]
    fn select_transport_noninteractive_uses_flag() {
        let a = args(false, true);
        let t = select_transport(&a, false).unwrap();
        assert_eq!(t, WizardTransport::Stdio);
    }
}
