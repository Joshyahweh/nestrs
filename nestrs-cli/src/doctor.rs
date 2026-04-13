//! `nestrs doctor` — toolchain + dependency sanity checks for nestrs apps.

use std::path::Path;
use std::process::Command;

/// Run from repo root or app crate directory (where `Cargo.toml` lives).
pub fn run() -> Result<(), String> {
    println!("nestrs doctor");
    println!();

    print_cmd_version("rustc", &["--version"])?;
    print_cmd_version("cargo", &["--version"])?;

    let manifest = Path::new("Cargo.toml");
    if !manifest.exists() {
        println!("hint: run from a directory containing Cargo.toml (no manifest here).");
        return Ok(());
    }

    let toml = std::fs::read_to_string(manifest).map_err(|e| e.to_string())?;
    println!();
    println!("Cargo.toml (nestrs-related hints):");
    summarize_nestrs_toml(&toml);

    if let Ok(out) = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
    {
        if out.status.success() {
            if let Ok(meta) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(packages) = meta.get("packages").and_then(|p| p.as_array()) {
                    for pkg in packages {
                        let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name != "nestrs" {
                            continue;
                        }
                        if let Some(feats) = pkg.get("features").and_then(|f| f.as_object()) {
                            println!();
                            println!(
                                "Resolved package `nestrs` has {} feature flags in metadata.",
                                feats.len()
                            );
                        }
                        break;
                    }
                }
            }
        }
    }

    println!();
    println!("Source scan (heuristic):");
    scan_src_hints(Path::new("src"), &toml)?;

    println!();
    println!("Done. This does not replace `cargo check` or security audits.");
    Ok(())
}

fn print_cmd_version(label: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(label)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run `{label}`: {e}"))?;
    if !out.status.success() {
        return Err(format!("`{label}` exited with {}", out.status));
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    println!("{label}: {line}");
    Ok(())
}

fn summarize_nestrs_toml(toml: &str) {
    let has_openapi = toml_contains_feature(toml, "openapi");
    let has_otel = toml_contains_feature(toml, "otel");
    let has_ws = toml_contains_feature(toml, "ws");
    let has_graphql = toml_contains_feature(toml, "graphql");
    let has_micro = toml_contains_feature(toml, "microservices");

    if toml.contains("nestrs") {
        println!("  - `nestrs` dependency found.");
        println!("    Features detected (string heuristic): openapi={has_openapi}, otel={has_otel}, ws={has_ws}, graphql={has_graphql}, microservices={has_micro}");
    } else {
        println!("  - No literal `nestrs` in Cargo.toml (workspace path deps / renames need manual review).");
    }
}

fn toml_contains_feature(toml: &str, feature: &str) -> bool {
    let needle = format!("\"{feature}\"");
    toml.contains(&needle)
}

fn scan_src_hints(dir: &Path, cargo_toml: &str) -> Result<(), String> {
    if !dir.is_dir() {
        println!("  - No `src/` directory; skipped.");
        return Ok(());
    }

    let mut buf = String::new();
    collect_rs_files(dir, &mut buf)?;

    let mut printed = false;

    if buf.contains("enable_openapi") && !toml_contains_feature(cargo_toml, "openapi") {
        printed = true;
        println!("  - [!] `enable_openapi()`-style call suggested but `openapi` feature not detected in Cargo.toml — add `features = [..., \"openapi\", ...]` for `nestrs`.");
    }
    if buf.contains("configure_tracing_opentelemetry") && !toml_contains_feature(cargo_toml, "otel")
    {
        printed = true;
        println!("  - [!] OpenTelemetry tracing API referenced but `otel` feature not detected — enable `features = [..., \"otel\", ...]`.");
    }
    if buf.contains("enable_graphql") && !toml_contains_feature(cargo_toml, "graphql") {
        printed = true;
        println!("  - [!] GraphQL bootstrap referenced but `graphql` feature not detected.");
    }

    if buf.contains("nestrs_openapi::") || buf.contains("nestrs::nestrs_openapi::") {
        printed = true;
        println!("  - [i] Uses `nestrs_openapi` paths — ensure `openapi` feature is enabled; prefer `nestrs::nestrs_openapi::OpenApiOptions` or prelude re-exports when applicable.");
    }

    if !printed {
        println!("  - No obvious feature mismatches from quick scan.");
    }

    Ok(())
}

fn collect_rs_files(dir: &Path, out: &mut String) -> Result<(), String> {
    let read = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    for ent in read.flatten() {
        let p = ent.path();
        if p.is_dir() {
            collect_rs_files(&p, out)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(s) = std::fs::read_to_string(&p) {
                out.push_str(&s);
                out.push('\n');
            }
        }
    }
    Ok(())
}
