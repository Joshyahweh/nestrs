//! Idempotent TOML merge.
//!
//! Reads an existing TOML config (if any), inserts or replaces the
//! `[<table>.<server_name>]` entry, and writes the result back atomically.
//! All other tables and keys are preserved.
//!
//! **Known limitation**: `toml::to_string_pretty` reorders tables relative
//! to the source. The set of keys is preserved; the order is not. The
//! wizard surfaces this with a one-line hint in the summary banner.

use std::fs;
use std::io;
use std::path::Path;

use anyhow::{bail, Context, Result};
use toml::Value;

use super::WriteOutcome;

/// Merge a single server entry into a TOML config file under `[<table>.<server_name>]`.
///
/// Outcome is the same as [`super::json_merge::merge_json`]:
/// [`WriteOutcome::Created`], [`WriteOutcome::Added`], [`WriteOutcome::Updated`],
/// or [`WriteOutcome::NoChange`].
pub fn merge_toml(
    path: &Path,
    table: &str,
    server_name: &str,
    server_value: Value,
) -> Result<WriteOutcome> {
    let existed = path.exists();
    let mut doc = match fs::read_to_string(path) {
        Ok(s) => {
            let v: Value = s
                .parse()
                .with_context(|| format!("failed to parse existing TOML at {}", path.display()))?;
            v
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Value::Table(Default::default()),
        Err(e) => {
            return Err(e).with_context(|| format!("failed to read {}", path.display()));
        }
    };

    // 1. Coerce root to Table.
    let root = match &mut doc {
        Value::Table(t) => t,
        other => bail!(
            "expected {} to contain a TOML table at the top level, found {}",
            path.display(),
            toml_type_name(other)
        ),
    };

    // 2. Get-or-create the named table.
    let entry = root
        .entry(table.to_string())
        .or_insert_with(|| Value::Table(Default::default()));

    let inner = match entry {
        Value::Table(t) => t,
        other => bail!(
            "expected `{}` in {} to be a TOML table, found {}. Fix the file manually and re-run.",
            table,
            path.display(),
            toml_type_name(other)
        ),
    };

    // 3. Insert/replace. Compare values for the no-change case.
    let prior = inner.insert(server_name.to_string(), server_value.clone());
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

    // 4. Ensure the parent dir exists.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
        }
    }

    // 5. Atomic write.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let serialized = toml::to_string_pretty(&doc)
        .with_context(|| format!("failed to re-serialize TOML for {}", path.display()))?;
    fs::write(&tmp, serialized).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("failed to rename {} -> {}", tmp.display(), path.display()))?;

    Ok(outcome)
}

fn toml_type_name(v: &Value) -> &'static str {
    match v {
        Value::Boolean(_) => "boolean",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::Datetime(_) => "datetime",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio_inline() -> Value {
        toml::toml! {
            command = "nestrs-mcp"
            args = []
        }
        .into()
    }

    #[test]
    fn nonexistent_file_creates_with_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        let outcome = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(outcome, WriteOutcome::Created);
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("[mcp_servers.nestrs]"), "got: {s}");
        assert!(s.contains("command = \"nestrs-mcp\""), "got: {s}");
        assert!(s.contains("args = []"), "got: {s}");
    }

    #[test]
    fn preserves_unrelated_tables() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[mcp_servers.other]
command = "x"

[model]
name = "gpt-5"
"#,
        )
        .unwrap();

        let outcome = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(outcome, WriteOutcome::Added);

        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("[mcp_servers.other]"), "other entry preserved: {s}");
        assert!(s.contains("command = \"x\""), "other command preserved: {s}");
        assert!(s.contains("[model]"), "unrelated table preserved: {s}");
        assert!(s.contains("name = \"gpt-5\""), "unrelated key preserved: {s}");
        assert!(s.contains("[mcp_servers.nestrs]"), "nestrs entry added: {s}");
    }

    #[test]
    fn replaces_existing_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[mcp_servers.nestrs]
url = "http://old/mcp"
"#,
        )
        .unwrap();

        let outcome = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(outcome, WriteOutcome::Updated);
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.contains("command = \"nestrs-mcp\""), "stdio shape written: {s}");
        assert!(!s.contains("http://old"), "old URL gone: {s}");
    }

    #[test]
    fn no_change_when_entry_byte_identical() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        let initial = "[mcp_servers.nestrs]\ncommand = \"nestrs-mcp\"\nargs = []\n";
        std::fs::write(&path, initial).unwrap();

        let before = std::fs::read(&path).unwrap();
        let outcome = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(outcome, WriteOutcome::NoChange);
        let after = std::fs::read(&path).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn invalid_toml_surfaces_parse_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, "this is = = not valid toml [[[").unwrap();
        let err = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("failed to parse existing TOML"),
            "got: {msg}"
        );
    }

    #[test]
    fn idempotent_across_runs() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("config.toml");

        let first = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(first, WriteOutcome::Created);
        let s1 = std::fs::read_to_string(&path).unwrap();

        let second = merge_toml(&path, "mcp_servers", "nestrs", stdio_inline()).unwrap();
        assert_eq!(second, WriteOutcome::NoChange);
        let s2 = std::fs::read_to_string(&path).unwrap();
        assert_eq!(s1, s2, "second run should not have rewritten the file");
    }
}
