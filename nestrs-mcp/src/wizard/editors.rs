//! Editor enumeration and detection.
//!
//! Each variant of [`Editor`] is a well-known MCP config location. The wizard
//! probes each variant's resolved path and returns the ones whose config file
//! already exists (or whose parent dir exists, so the wizard can offer the
//! editor even before the user has saved anything to it).

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One supported editor's MCP config location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Editor {
    /// Claude Code (project-local): `./.mcp.json`, top-level key `mcpServers`.
    ClaudeCodeProject,
    /// Claude Code (global): `~/.claude.json`, top-level key `mcpServers`.
    ClaudeCodeGlobal,
    /// Cursor: `~/.cursor/mcp.json`, top-level key `mcpServers`.
    Cursor,
    /// VS Code GitHub Copilot Chat: `./.vscode/mcp.json`, top-level key `servers`.
    VsCodeCopilot,
    /// Codex CLI: `~/.codex/config.toml`, top-level TOML table `[mcp_servers]`.
    Codex,
}

impl Editor {
    /// All known editors, in the order the wizard should display them.
    pub const fn all() -> [Editor; 5] {
        [
            Editor::ClaudeCodeProject,
            Editor::ClaudeCodeGlobal,
            Editor::Cursor,
            Editor::VsCodeCopilot,
            Editor::Codex,
        ]
    }

    /// Resolved absolute path to this editor's config file.
    pub fn path(self, cwd: &Path) -> Result<PathBuf> {
        let p = match self {
            Editor::ClaudeCodeProject => cwd.join(".mcp.json"),
            Editor::ClaudeCodeGlobal => home_dir()?.join(".claude.json"),
            Editor::Cursor => home_dir()?.join(".cursor").join("mcp.json"),
            Editor::VsCodeCopilot => cwd.join(".vscode").join("mcp.json"),
            Editor::Codex => home_dir()?.join(".codex").join("config.toml"),
        };
        Ok(p)
    }

    /// The JSON top-level key (only used for JSON editors; TOML editors
    /// return the table name via `toml_table`).
    pub fn top_level_key(self) -> &'static str {
        match self {
            Editor::VsCodeCopilot => "servers",
            _ => "mcpServers",
        }
    }

    /// The TOML table name (only meaningful for TOML editors).
    pub fn toml_table(self) -> &'static str {
        "mcp_servers"
    }

    /// Whether this editor's config is TOML (vs JSON).
    pub fn is_toml(self) -> bool {
        matches!(self, Editor::Codex)
    }

    /// Human label for the editor (used in the multi-select prompt).
    pub fn label(self) -> &'static str {
        match self {
            Editor::ClaudeCodeProject => "Claude Code (project)",
            Editor::ClaudeCodeGlobal => "Claude Code (global)",
            Editor::Cursor => "Cursor",
            Editor::VsCodeCopilot => "VS Code (Copilot)",
            Editor::Codex => "Codex CLI",
        }
    }
}

impl fmt::Display for Editor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Resolved `(editor, absolute path)` pair. The wizard uses this when it
/// needs to render the path in the multi-select prompt or in the write log.
#[derive(Debug, Clone)]
pub struct Entry {
    pub editor: Editor,
    pub path: PathBuf,
}

/// Resolve every known editor to an absolute path. Resolution never fails
/// for the project-relative editors (they're under `cwd`). It can fail for
/// the global editors if `dirs::home_dir()` returns `None` (no home dir on
/// the system), in which case the global entries are skipped.
pub fn resolve_all(cwd: &Path) -> Vec<Entry> {
    Editor::all()
        .iter()
        .filter_map(|&e| e.path(cwd).ok().map(|p| Entry { editor: e, path: p }))
        .collect()
}

/// Probe every known editor and return the ones that look "installed" —
/// i.e. the config file already exists, or its parent directory exists
/// (so we can offer the editor before the user has ever opened it).
pub fn detect(cwd: &Path) -> Vec<Editor> {
    resolve_all(cwd)
        .into_iter()
        .filter(|e| looks_installed(&e.path))
        .map(|e| e.editor)
        .collect()
}

fn looks_installed(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    // No file yet, but the parent dir exists — offer this editor so the
    // user can still pick it. The merge will create the file.
    path.parent().map(|p| p.exists()).unwrap_or(false)
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn editor_path_resolution_is_absolute_and_uses_home_for_globals() {
        let temp = TempDir::new().unwrap();
        let cwd = temp.path();

        // Project-relative editors sit under cwd.
        let p = Editor::ClaudeCodeProject.path(cwd).unwrap();
        assert!(p.is_absolute());
        assert!(p.starts_with(cwd));
        assert!(p.ends_with(".mcp.json"));

        let p = Editor::VsCodeCopilot.path(cwd).unwrap();
        assert!(p.is_absolute());
        assert!(p.starts_with(cwd));
        assert!(p.ends_with(".vscode/mcp.json"));

        // Global editors (if home is resolvable) sit under the home dir.
        if let Some(home) = dirs::home_dir() {
            for (editor, suffix) in [
                (Editor::ClaudeCodeGlobal, ".claude.json"),
                (Editor::Cursor, ".cursor/mcp.json"),
                (Editor::Codex, ".codex/config.toml"),
            ] {
                let p = editor.path(cwd).unwrap();
                assert!(p.is_absolute(), "{editor:?} path should be absolute");
                assert!(
                    p.starts_with(&home),
                    "{editor:?} path {p:?} should start with home {home:?}"
                );
                assert!(
                    p.ends_with(suffix),
                    "{editor:?} path should end with {suffix}"
                );
            }
        }
    }

    #[test]
    fn top_level_key_matches_docs() {
        assert_eq!(Editor::ClaudeCodeProject.top_level_key(), "mcpServers");
        assert_eq!(Editor::ClaudeCodeGlobal.top_level_key(), "mcpServers");
        assert_eq!(Editor::Cursor.top_level_key(), "mcpServers");
        assert_eq!(Editor::VsCodeCopilot.top_level_key(), "servers");
        assert_eq!(Editor::Codex.toml_table(), "mcp_servers");
    }

    #[test]
    fn only_codex_is_toml() {
        for e in Editor::all() {
            assert_eq!(e.is_toml(), matches!(e, Editor::Codex));
        }
    }

    #[test]
    fn detect_finds_existing_config_file() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join(".mcp.json"), "{}").unwrap();
        let detected = detect(temp.path());
        assert!(detected.contains(&Editor::ClaudeCodeProject));
    }

    #[test]
    fn detect_finds_editor_whose_parent_dir_exists_but_file_does_not() {
        let temp = TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join(".vscode")).unwrap();
        let detected = detect(temp.path());
        assert!(
            detected.contains(&Editor::VsCodeCopilot),
            "VS Code should be detected when .vscode/ exists, got {detected:?}"
        );
    }
}
