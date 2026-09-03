//! Idempotent JSON merge.
//!
//! Reads an existing JSON config (if any), inserts or replaces the
//! `mcpServers.<server_name>` (or `servers.<server_name>`) entry, and
//! writes the result back atomically. All other top-level keys and all
//! other entries under the top-level key are preserved.

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

use super::WriteOutcome;

/// Merge a single server entry into a JSON config file.
///
/// - If `path` does not exist, creates it with just the `nestrs` entry under
///   `top_level_key`. Returns [`WriteOutcome::Created`].
/// - If `path` exists and has no prior `nestrs` entry, adds it. Returns
///   [`WriteOutcome::Added`].
/// - If `path` exists and has a prior `nestrs` entry, replaces it. Returns
///   [`WriteOutcome::Updated`].
/// - If the existing `nestrs` entry is byte-identical to the new one (after
///   pretty-printing), returns [`WriteOutcome::NoChange`] and skips the
///   write.
pub fn merge_json(
    path: &Path,
    top_level_key: &str,
    server_name: &str,
    server_value: Value,
) -> Result<WriteOutcome> {
    let existed = path.exists();
    let root = match fs::read_to_string(path) {
        Ok(s) => Value::Object(
            serde_json::from_str::<Map<String, Value>>(&s)
                .with_context(|| format!("failed to parse existing JSON at {}", path.display()))?,
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Value::Object(Map::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read {}", path.display()));
        }
    };

    // 1. Coerce root to Object.
    let mut root_map = match root {
        Value::Object(m) => m,
        other => bail!(
            "expected {} to contain a JSON object at the top level, found {}",
            path.display(),
            json_type_name(&other)
        ),
    };

    // 2. Get-or-create the top-level key.
    let servers_entry = root_map
        .entry(top_level_key.to_string())
        .or_insert_with(|| Value::Object(Map::new()));

    // 3. Coerce top-level key to Object.
    let servers = match servers_entry {
        Value::Object(m) => m,
        other => bail!(
            "expected `{}` in {} to be a JSON object, found {}. Fix the file manually and re-run.",
            top_level_key,
            path.display(),
            json_type_name(other)
        ),
    };

    // 4. Insert/replace the server entry. We need the prior value to
    //    distinguish "no change" from "replaced".
    let prior = servers.insert(server_name.to_string(), server_value.clone());

    // 5. Decide the outcome *before* the write so we can short-circuit on
    //    no-change.
    let outcome = if !existed {
        WriteOutcome::Created
    } else if prior.is_none() {
        WriteOutcome::Added
    } else if prior.as_ref() == Some(&server_value) {
        WriteOutcome::NoChange
    } else {
        WriteOutcome::Updated
    };

    if matches!(outcome, WriteOutcome::NoChange) {
        return Ok(outcome);
    }

    // 6. Ensure the parent dir exists (Claude Code project-local, VS Code
    //    project-local) so the write doesn't fail on a fresh checkout.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
        }
    }

    // 7. Atomic write: temp file in the same dir, then rename.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let serialized = serde_json::to_string_pretty(&Value::Object(root_map))?;
    fs::write(&tmp, serialized).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()))?;

    Ok(outcome)
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn stdio_value() -> Value {
        serde_json::json!({ "command": "nestrs-mcp", "args": [] })
    }

    #[test]
    fn nonexistent_file_creates_with_entry() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        let outcome = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap();
        assert_eq!(outcome, WriteOutcome::Created);
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["nestrs"], stdio_value());
    }

    #[test]
    fn preserves_other_servers() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        let initial = serde_json::json!({
            "mcpServers": {
                "other": { "command": "x" },
                "third": { "url": "http://y" }
            },
            "user": { "theme": "dark" }
        });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let outcome = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap();
        assert_eq!(outcome, WriteOutcome::Added);

        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["mcpServers"]["other"],
            serde_json::json!({ "command": "x" })
        );
        assert_eq!(
            v["mcpServers"]["third"],
            serde_json::json!({ "url": "http://y" })
        );
        assert_eq!(v["user"]["theme"], "dark");
        assert_eq!(v["mcpServers"]["nestrs"], stdio_value());
    }

    #[test]
    fn uses_servers_key_for_vscode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        let http_value = serde_json::json!({ "type": "http", "url": "http://x/mcp" });
        let outcome = merge_json(&path, "servers", "nestrs", http_value.clone()).unwrap();
        assert_eq!(outcome, WriteOutcome::Created);
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["servers"]["nestrs"], http_value);
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn replaces_existing_entry_and_reports_updated() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        let old = serde_json::json!({ "mcpServers": { "nestrs": { "url": "http://old/mcp" } } });
        fs::write(&path, serde_json::to_string_pretty(&old).unwrap()).unwrap();

        let outcome = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap();
        assert_eq!(outcome, WriteOutcome::Updated);
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["nestrs"], stdio_value());
    }

    #[test]
    fn no_change_when_entry_byte_identical() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        let initial = serde_json::json!({ "mcpServers": { "nestrs": stdio_value() } });
        fs::write(&path, serde_json::to_string_pretty(&initial).unwrap()).unwrap();

        let before = fs::read(&path).unwrap();
        let outcome = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap();
        assert_eq!(outcome, WriteOutcome::NoChange);
        // File should be untouched.
        let after = fs::read(&path).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn invalid_json_surfaces_parse_error() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        fs::write(&path, "{ this is not json").unwrap();
        let err = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("failed to parse existing JSON"), "got: {msg}");
    }

    #[test]
    fn wrong_top_level_shape_errors() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("mcp.json");
        fs::write(&path, r#"{ "mcpServers": "oops" }"#).unwrap();
        let err = merge_json(&path, "mcpServers", "nestrs", stdio_value()).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("expected `mcpServers`"),
            "expected message to mention `mcpServers`, got: {msg}"
        );
        assert!(
            msg.contains("found string"),
            "expected message to mention `found string`, got: {msg}"
        );
    }

    #[test]
    fn creates_parent_dir_for_project_local_files() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join(".vscode").join("mcp.json");
        // .vscode does not exist yet.
        assert!(!path.parent().unwrap().exists());
        let outcome = merge_json(&path, "servers", "nestrs", stdio_value()).unwrap();
        assert_eq!(outcome, WriteOutcome::Created);
        assert!(path.exists());
    }
}
