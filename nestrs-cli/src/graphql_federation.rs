//! Wave 7.12 — `nestrs-cli graphql federation export`.
//!
//! Thin wrapper over [`crate::graphql_sdl::fetch_sdl`] that fetches
//! the federation v2 subgraph SDL from a running endpoint and
//! validates that the response is actually a federation-v2 SDL
//! (contains the `@link` directive) before writing it to disk.
//!
//! Federation v2 is the Apollo Federation shape that nestrs-graphql
//! emits via `export_subgraph_v2_sdl` (re-exported by the umbrella as
//! `nestrs::graphql::export_subgraph_v2_sdl`). HTTP SDL export is
//! federation-only (`{_service { sdl }}`); non-federation schemas
//! should print SDL at build time.
//! Running against a federation-v1 or non-federation endpoint by
//! mistake is a common CI failure — the CLI surfaces it as an error
//! rather than silently writing a broken SDL.
//!
//! Transport: shell out to `curl` via `graphql_sdl::fetch_sdl`. The
//! CLI stays light (no Rust HTTP client dep).

use std::path::PathBuf;

use crate::graphql_sdl;

/// Entry point for `nestrs-cli graphql federation export`.
/// Args after `graphql federation export`:
/// `[--url <http>] [--out <path>] [--bearer-token <token>] [--lenient]`.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut url: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut bearer: Option<String> = None;
    // Default = strict (federation v2 only). `--lenient` accepts v1
    // SDLs that lack the `@link` directive.
    let mut lenient = false;
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
            "--lenient" => {
                lenient = true;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let url = url.ok_or_else(|| "missing required `--url <http>`".to_string())?;
    let out = out.ok_or_else(|| "missing required `--out <path>`".to_string())?;

    let sdl = graphql_sdl::fetch_sdl(&url, bearer.as_deref(), true)?;

    if !lenient && !is_federation_v2_sdl(&sdl) {
        return Err(format!(
            "endpoint {url} did not return a federation v2 SDL \
             (no `@link` directive found). Re-run with `--lenient` to \
             accept a federation v1 / non-federation SDL, or point the \
             CLI at the correct federation v2 subgraph."
        ));
    }

    graphql_sdl::write_sdl(&out, &sdl)?;
    let kind = if is_federation_v2_sdl(&sdl) {
        "federation v2"
    } else {
        "federation v1 / non-federation (--lenient)"
    };
    println!(
        "wrote {} bytes of {} SDL to {}",
        sdl.len(),
        kind,
        out.display()
    );
    Ok(())
}

/// Detect whether a federation v2 SDL string was emitted by
/// `nestrs_graphql::export_subgraph_v2_sdl`. Mirrors the helper of
/// the same name in `nestrs-graphql::federation` — duplicated here
/// because the CLI does not depend on `nestrs-graphql` (the check is
/// a one-liner substring match, not worth pulling the dep).
///
/// Returns `true` if the SDL contains the federation v2 `@link`
/// directive. The federation v1 SDL never contains `@link`; v2
/// always does.
pub fn is_federation_v2_sdl(sdl: &str) -> bool {
    sdl.contains("@link")
}

/// Sub-dispatch entry point wired into `main.rs` as
/// `"graphql" => ... "federation" => graphql_federation::dispatch(...)`.
/// Routes `nestrs-cli graphql federation <subcommand>` to the right
/// implementation. Currently only `export` is implemented; future
/// subcommands (`validate`, `compose`, `plan`) would slot in here.
pub fn dispatch(args: &[String]) -> Result<(), String> {
    match args.first().map(|s| s.as_str()) {
        Some("export") => run(&args[1..]),
        Some(other) => Err(format!(
            "unknown `nestrs-cli graphql federation {other}` subcommand; \
             currently only `export` is implemented"
        )),
        None => Err("expected `nestrs-cli graphql federation <subcommand>` \
             (currently: `export`)"
            .to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_federation_v2_sdl_detects_at_link() {
        let sdl = "directive @link(url: String) on FIELD_DEFINITION\ntype Query { ping: String }\n";
        assert!(is_federation_v2_sdl(sdl));
    }

    #[test]
    fn is_federation_v2_sdl_rejects_v1_or_plain() {
        assert!(!is_federation_v2_sdl("type Query { ping: String }\n"));
        assert!(!is_federation_v2_sdl(
            "type User @key(fields: \"id\") { id: ID! }\ntype Query { me: User }\n"
        ));
    }

    #[test]
    fn is_federation_v2_sdl_handles_empty_input() {
        assert!(!is_federation_v2_sdl(""));
    }
}
