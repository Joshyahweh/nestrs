//! Wave 7.11 — `nestrs-cli graphql sdl` federation-aware SDL exporter.
//!
//! For federation-enabled apps, the SDL is exposed at runtime via the
//! `_service { sdl }` introspection field (Apollo Federation v2). This
//! subcommand POSTs the query, parses the JSON response, and writes
//! the SDL string to disk.
//!
//! For non-federation apps, print SDL at **build time** with
//! `export_schema_sdl` / `export_sdl_to_file` in the user's binary.
//! `--no-federation` runs standard GraphQL introspection and does
//! **not** return Schema Definition Language.
//!
//! Transport: shell out to `curl`. Avoids pulling a heavy HTTP-client
//! dependency into the CLI; every macOS / Linux dev box has `curl`.
//! `--url` is the GraphQL endpoint; `--out` is the file to write; the
//! HTTP method is forced to POST with `Content-Type: application/json`.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SdlResponse {
    data: Option<SdlData>,
}

#[derive(Debug, Deserialize)]
struct SdlData {
    #[serde(rename = "_service")]
    service: Option<ServiceField>,
}

#[derive(Debug, Deserialize)]
struct ServiceField {
    sdl: Option<String>,
}

/// Entry point for `nestrs-cli graphql sdl`. Args after `graphql sdl`:
/// `[--url <http>] [--out <path>] [--bearer-token <token>]`.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut url: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut bearer: Option<String> = None;
    let mut federation = true;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url = Some(
                    args.get(i)
                        .ok_or_else(|| "missing value for --url".to_string())?
                        .clone(),
                );
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(
                    args.get(i)
                        .ok_or_else(|| "missing value for --out".to_string())?,
                ));
            }
            "--bearer-token" => {
                i += 1;
                bearer = Some(
                    args.get(i)
                        .ok_or_else(|| "missing value for --bearer-token".to_string())?
                        .clone(),
                );
            }
            "--no-federation" => {
                federation = false;
            }
            "--federation" => {
                federation = true;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let url = url.ok_or_else(|| "missing required `--url <http>`".to_string())?;
    let out = out.ok_or_else(|| "missing required `--out <path>`".to_string())?;

    let sdl = fetch_sdl(&url, bearer.as_deref(), federation)?;
    write_sdl(&out, &sdl)?;
    println!("wrote {} bytes of SDL to {}", sdl.len(), out.display());
    Ok(())
}

/// Query the GraphQL endpoint for `{ _service { sdl } }` (federation)
/// or `{ __schema { types { name } } }` introspection fallback. Returns
/// the SDL string. Parses the JSON response via `serde_json`.
pub fn fetch_sdl(url: &str, bearer: Option<&str>, federation: bool) -> Result<String, String> {
    let query = if federation {
        r#"{"query":"{_service{sdl}}"}"#
    } else {
        // Non-federation: standard introspection query for the
        // canonical SDL. We pull `__schema { types { name kind } }`
        // and rebuild a minimal SDL by name listing — this is the
        // federation gateway's fallback when the running schema is
        // not federation-enabled. For richer SDL output, the user
        // should run with `--federation` against a federation
        // subgraph, or call `export_schema_sdl` at build time.
        r#"{"query":"{__schema{types{name}}}"}"#
    };

    let mut cmd = Command::new("curl");
    cmd.arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("--data")
        .arg(query)
        .arg("--max-time")
        .arg("15");
    if let Some(token) = bearer {
        cmd.arg("-H").arg(format!("Authorization: Bearer {token}"));
    }
    // End option parsing so a URL that starts with `-` cannot be
    // interpreted as extra curl flags.
    cmd.arg("--").arg(url);
    cmd.stdin(Stdio::null());

    let output = cmd.output().map_err(|e| {
        format!("failed to invoke `curl` against {url}: {e}. Is `curl` on your $PATH?")
    })?;
    if !output.status.success() {
        return Err(format!(
            "curl exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let body = String::from_utf8_lossy(&output.stdout).to_string();
    if federation {
        let parsed: SdlResponse = serde_json::from_str(&body)
            .map_err(|e| format!("response was not valid JSON: {e}; body = {body}"))?;
        let sdl = parsed
            .data
            .and_then(|d| d.service)
            .and_then(|s| s.sdl)
            .ok_or_else(|| {
                format!(
                    "response did not contain `_service {{ sdl }}`. \
                     Body: {body}"
                )
            })?;
        Ok(sdl)
    } else {
        // Non-federation: the SDL isn't recoverable from introspection
        // alone — return a placeholder that tells the user how to get
        // the real one. We still parse the JSON to confirm the
        // endpoint is alive.
        let parsed: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("response was not valid JSON: {e}; body = {body}"))?;
        if parsed.get("errors").is_some() {
            return Err(format!(
                "introspection query returned errors: {}",
                parsed["errors"]
            ));
        }
        if parsed.get("data").is_none() {
            return Err(format!("introspection query returned no `data`: {body}"));
        }
        Err(
            "non-federation SDL export via HTTP is not supported — call \
             `nestrs_graphql::export_schema_sdl(&schema)` at build time, or \
             re-run with `--federation` against a federation-enabled subgraph"
                .to_string(),
        )
    }
}

/// Write the SDL string to disk. Creates parent directories if needed.
pub fn write_sdl(path: &PathBuf, sdl: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut f = fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(sdl.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Pure helper used by tests — fetch SDL from a synthetic body
/// without going through `curl`. Verifies the JSON-shape parsing in
/// isolation.
pub fn parse_sdl_body(body: &str) -> Result<String, String> {
    let parsed: SdlResponse =
        serde_json::from_str(body).map_err(|e| format!("response was not valid JSON: {e}"))?;
    parsed
        .data
        .and_then(|d| d.service)
        .and_then(|s| s.sdl)
        .ok_or_else(|| "response did not contain `_service { sdl }`".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sdl_body_extracts_federation_sdl() {
        let body = r#"{"data":{"_service":{"sdl":"type User { id: ID! }"}}}"#;
        let sdl = parse_sdl_body(body).expect("parse");
        assert_eq!(sdl, "type User { id: ID! }");
    }

    #[test]
    fn parse_sdl_body_rejects_missing_data() {
        let body = r#"{"errors":[{"message":"not authorized"}]}"#;
        let err = parse_sdl_body(body).expect_err("should fail");
        assert!(err.contains("not contain"));
    }

    #[test]
    fn parse_sdl_body_rejects_malformed_json() {
        let body = "not json";
        let err = parse_sdl_body(body).expect_err("should fail");
        assert!(err.contains("not valid JSON"));
    }
}
