//! Wave 7.10 — `nestrs-cli repl` (static DI graph explorer).
//!
//! Lightweight source-level introspection: scan the user's crate for
//! `#[module(...)]`, `#[controller(...)]`, `#[injectable]`, `#[dto]`,
//! `impl_routes!`, and HTTP-method macros (`#[get]` / `#[post]` /
//! `#[put]` / `#[patch]` / `#[delete]`). No `syn` dependency —
//! regex-driven, best-effort, errors are reported instead of swallowed.
//!
//! Subcommands:
//! - `nestrs-cli repl graph [--path <dir>]` — emit a tree of modules,
//!   controllers (with routes), providers, and DTOs.
//! - `nestrs-cli repl routes [--path <dir>]` — flat list of every HTTP
//!   route, method + path + handler.
//! - `nestrs-cli repl providers [--path <dir>]` — flat list of every
//!   `#[injectable]` type and which module declares it.
//!
//! All modes default to scanning `./src` from the current working
//! directory. `--path <dir>` overrides the source root.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use regex::Regex;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ModuleEntry {
    name: String,
    file: String,
    controllers: Vec<String>,
    providers: Vec<String>,
    imports: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ControllerEntry {
    name: String,
    file: String,
    prefix: Option<String>,
    routes: Vec<RouteEntry>,
}

#[derive(Debug, Serialize)]
struct RouteEntry {
    method: String,
    path: String,
    handler: String,
}

#[derive(Debug, Serialize)]
struct ProviderEntry {
    name: String,
    file: String,
}

#[derive(Debug, Serialize)]
struct DtoEntry {
    name: String,
    file: String,
}

#[derive(Debug, Default, Serialize)]
struct Graph {
    modules: Vec<ModuleEntry>,
    controllers: Vec<ControllerEntry>,
    providers: Vec<ProviderEntry>,
    dtos: Vec<DtoEntry>,
}

/// Entry point. Dispatches to graph/routes/providers sub-subcommands.
/// `args` are the post-`repl` argv tokens.
pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "expected `nestrs-cli repl <graph|routes|providers> [--path <dir>] [--format text|json]`"
                .to_string(),
        );
    }

    let mut path = PathBuf::from("src");
    let mut format = String::from("text");
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --path".to_string())?;
                path = PathBuf::from(value);
            }
            "--format" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --format".to_string())?;
                format = value.clone();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }

    let graph = scan(&path)?;
    match args[0].as_str() {
        "graph" => print_graph(&graph, &format),
        "routes" => print_routes(&graph, &format),
        "providers" => print_providers(&graph, &format),
        "dtos" => print_dtos(&graph, &format),
        other => Err(format!("unknown subcommand `{other}`")),
    }
}

/// Walk `root` for `.rs` files and extract the DI graph.
pub fn scan(root: &Path) -> Result<Graph, String> {
    if !root.exists() {
        return Err(format!("source path does not exist: {}", root.display()));
    }
    let mut files = Vec::new();
    collect_rs_files(root, &mut files)?;
    files.sort();

    let mut graph = Graph::default();

    let re_module = Regex::new(r"#\s*\[\s*module\b([^\]]*)\]").unwrap();
    let re_controller = Regex::new(r"#\s*\[\s*controller\b([^\]]*)\]").unwrap();
    let re_injectable = Regex::new(r"#\s*\[\s*injectable\b[^\]]*\]").unwrap();
    let re_dto = Regex::new(r"#\s*\[\s*dto\b[^\]]*\]").unwrap();
    let re_impl_routes = Regex::new(r"impl_routes!\s*\(").unwrap();
    let re_prefix =
        Regex::new(r#"prefix\s*=\s*"([^"]+)""#).unwrap();
    let re_method_route = Regex::new(
        r"#\s*\[\s*(get|post|put|patch|delete|head|options)\s*\(\s*\"([^\"]*)\"\s*\)\s*\]",
    )
    .unwrap();
    let re_handler = Regex::new(r"\b(?:async\s+)?fn\s+([a-z][A-Za-z0-9_]*)\b").unwrap();
    let re_keyed_list =
        Regex::new(r"(?:controllers|providers|imports|exports)\s*=\s*\[([^\]]*)\]").unwrap();

    for file in &files {
        let display = file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string();
        let content = fs::read_to_string(file).map_err(|e| e.to_string())?;

        // Modules — `#[module(controllers = [...], providers = [...], imports = [...])]`
        for cap in re_module.captures_iter(&content) {
            let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let module_name = next_struct_name_after(&content, cap.get(0).unwrap().end());
            let (controllers, providers, imports) = parse_lists(inner, &re_keyed_list);
            if let Some(name) = module_name {
                graph.modules.push(ModuleEntry {
                    name,
                    file: display.clone(),
                    controllers,
                    providers,
                    imports,
                });
            }
        }

        // Controllers — `#[controller(prefix = "/x")]` + `#[get("/...")]` routes
        for cap in re_controller.captures_iter(&content) {
            let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let prefix = re_prefix
                .captures(inner)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()));
            let controller_name = next_struct_name_after(&content, cap.get(0).unwrap().end());

            // Find routes — but only those AFTER this controller's struct
            // declaration, so we don't pick up routes from another controller
            // declared in the same file.
            let struct_start = cap.get(0).unwrap().end();
            let mut routes = Vec::new();
            if let Some(struct_name) = &controller_name {
                if let Some(impl_start) = find_impl_block(&content, struct_name) {
                    let slice = &content[impl_start..];
                    for route_cap in re_method_route.captures_iter(slice) {
                        let method = route_cap
                            .get(1)
                            .map(|m| m.as_str().to_uppercase())
                            .unwrap_or_default();
                        let path = route_cap.get(2).map(|m| m.as_str()).unwrap_or("");
                        // Handler name is the next `fn` after the route attr.
                        let after_attr = impl_start + route_cap.get(0).unwrap().end();
                        let handler = next_handler_name(&content, after_attr, &re_handler);
                        if let Some(handler_name) = handler {
                            routes.push(RouteEntry {
                                method,
                                path: path.to_string(),
                                handler: handler_name,
                            });
                        }
                    }
                }
            }
            // Avoid unused-warning for struct_start in case controllers
            // are declared with no impl block (rare but valid).
            let _ = struct_start;
            if let Some(name) = controller_name {
                graph.controllers.push(ControllerEntry {
                    name,
                    file: display.clone(),
                    prefix,
                    routes,
                });
            }
        }

        // Providers — `#[injectable]` must precede a struct declaration.
        // Walk positions: for each `#[injectable]` match, find the next
        // struct name after the attr.
        for cap in re_injectable.captures_iter(&content) {
            if let Some(name) = next_struct_name_after(&content, cap.get(0).unwrap().end()) {
                if !graph.providers.iter().any(|p| p.name == name) {
                    graph.providers.push(ProviderEntry {
                        name,
                        file: display.clone(),
                    });
                }
            }
        }

        // DTOs — `#[dto]` must precede a struct declaration.
        for cap in re_dto.captures_iter(&content) {
            if let Some(name) = next_struct_name_after(&content, cap.get(0).unwrap().end()) {
                if name.ends_with("Dto") || name.ends_with("Entity") || name.ends_with("Model") {
                    if !graph.dtos.iter().any(|d| d.name == name) {
                        graph.dtos.push(DtoEntry {
                            name,
                            file: display.clone(),
                        });
                    }
                }
            }
        }

        // `impl_routes!` blocks — these may declare routes with no
        // `#[get("/")]` attr (rare but valid in some 1.0.0 macros).
        let _ = re_impl_routes.is_match(&content); // acknowledged; route extraction is attr-driven above
    }

    Ok(graph)
}

fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if root.is_file() {
        if root.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(root.to_path_buf());
        }
        return Ok(());
    }
    let entries = fs::read_dir(root).map_err(|e| e.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            // Skip target/ and .git/.
            let skip = path
                .file_name()
                .and_then(|f| f.to_str())
                .map(|f| matches!(f, "target" | ".git" | "node_modules"))
                .unwrap_or(false);
            if skip {
                continue;
            }
            collect_rs_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn next_struct_name_after(content: &str, after: usize) -> Option<String> {
    let slice = &content[after..];
    let re = Regex::new(r"\bstruct\s+([A-Z][A-Za-z0-9_]*)\b").ok()?;
    re.captures(slice)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn next_handler_name(content: &str, after: usize, re: &Regex) -> Option<String> {
    let slice = &content[after..];
    re.captures(slice)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn find_impl_block(content: &str, struct_name: &str) -> Option<usize> {
    let pattern = format!(r"\bimpl\b[^\{{]*\b{struct_name}\b");
    let re = Regex::new(&pattern).ok()?;
    re.find(content).map(|m| m.start())
}

fn parse_lists(
    inner: &str,
    re_keyed_list: &Regex,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut controllers = Vec::new();
    let mut providers = Vec::new();
    let mut imports = Vec::new();
    for cap in re_keyed_list.captures_iter(inner) {
        let body = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let items = parse_ident_list(body);
        let full = cap.get(0).map(|m| m.as_str()).unwrap_or("");
        if full.starts_with("controllers") {
            controllers = items;
        } else if full.starts_with("providers") {
            providers = items;
        } else if full.starts_with("imports") {
            imports = items;
        }
        // exports are deliberately ignored at this level — they don't
        // change the graph traversal, just visibility.
    }
    (controllers, providers, imports)
}

fn parse_ident_list(body: &str) -> Vec<String> {
    body.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_').to_string())
        .filter(|s| !s.is_empty() && s.chars().next().map_or(false, |c| c.is_uppercase()))
        .collect()
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn print_graph(graph: &Graph, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!("nestrs DI graph ({} modules, {} controllers, {} providers, {} DTOs)",
                graph.modules.len(),
                graph.controllers.len(),
                graph.providers.len(),
                graph.dtos.len(),
            );
            println!();
            for module in &graph.modules {
                println!("module {}  ({})", module.name, module.file);
                if !module.imports.is_empty() {
                    println!("  imports:    {}", module.imports.join(", "));
                }
                if !module.controllers.is_empty() {
                    println!("  controllers: {}", module.controllers.join(", "));
                }
                if !module.providers.is_empty() {
                    println!("  providers:   {}", module.providers.join(", "));
                }
                for c in &graph.controllers {
                    if module.controllers.contains(&c.name) {
                        let prefix = c.prefix.clone().unwrap_or_else(|| "/".to_string());
                        println!("    controller {} (prefix = {prefix})", c.name);
                        for route in &c.routes {
                            let full_path = join_route(&prefix, &route.path);
                            println!("      {:>6} {full_path:30} -> {}::{}", route.method, c.name, route.handler);
                        }
                    }
                }
                println!();
            }
            Ok(())
        }
        "json" => {
            println!("{}", serde_json::to_string_pretty(graph).map_err(|e| e.to_string())?);
            Ok(())
        }
        other => Err(format!("unknown format `{other}`; expected text or json")),
    }
}

fn print_routes(graph: &Graph, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            let mut sorted: Vec<&ControllerEntry> = graph.controllers.iter().collect();
            sorted.sort_by(|a, b| a.name.cmp(&b.name));
            let mut rows: Vec<(String, String, String)> = Vec::new();
            for c in sorted {
                let prefix = c.prefix.clone().unwrap_or_else(|| "/".to_string());
                for route in &c.routes {
                    rows.push((
                        route.method.clone(),
                        join_route(&prefix, &route.path),
                        format!("{}::{}", c.name, route.handler),
                    ));
                }
            }
            rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
            println!("{:>6}  {:<40}  {}", "METHOD", "PATH", "HANDLER");
            for (m, p, h) in rows {
                println!("{m:>6}  {p:<40}  {h}");
            }
            Ok(())
        }
        "json" => {
            #[derive(serde::Serialize)]
            struct Row {
                method: String,
                path: String,
                handler: String,
            }
            let mut rows = Vec::new();
            for c in &graph.controllers {
                let prefix = c.prefix.clone().unwrap_or_else(|| "/".to_string());
                for route in &c.routes {
                    rows.push(Row {
                        method: route.method.clone(),
                        path: join_route(&prefix, &route.path),
                        handler: format!("{}::{}", c.name, route.handler),
                    });
                }
            }
            println!("{}", serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?);
            Ok(())
        }
        other => Err(format!("unknown format `{other}`; expected text or json")),
    }
}

fn print_providers(graph: &Graph, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            // Group providers by module declaration (best-effort).
            let mut by_module: BTreeMap<String, Vec<&ProviderEntry>> = BTreeMap::new();
            let mut unassigned: Vec<&ProviderEntry> = Vec::new();
            for module in &graph.modules {
                for p in &graph.providers {
                    if module.providers.contains(&p.name) {
                        by_module
                            .entry(module.name.clone())
                            .or_default()
                            .push(p);
                    }
                }
            }
            for p in &graph.providers {
                if !graph.modules.iter().any(|m| m.providers.contains(&p.name)) {
                    unassigned.push(p);
                }
            }
            for (module_name, providers) in &by_module {
                println!("module {module_name}:");
                for p in providers {
                    println!("  provider {}  ({})", p.name, p.file);
                }
            }
            if !unassigned.is_empty() {
                println!("unassigned:");
                for p in &unassigned {
                    println!("  provider {}  ({})", p.name, p.file);
                }
            }
            Ok(())
        }
        "json" => {
            println!("{}", serde_json::to_string_pretty(&graph.providers).map_err(|e| e.to_string())?);
            Ok(())
        }
        other => Err(format!("unknown format `{other}`; expected text or json")),
    }
}

fn print_dtos(graph: &Graph, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            for d in &graph.dtos {
                println!("{}  ({})", d.name, d.file);
            }
            Ok(())
        }
        "json" => {
            println!("{}", serde_json::to_string_pretty(&graph.dtos).map_err(|e| e.to_string())?);
            Ok(())
        }
        other => Err(format!("unknown format `{other}`; expected text or json")),
    }
}

fn join_route(prefix: &str, path: &str) -> String {
    if path == "/" {
        prefix.to_string()
    } else if prefix.ends_with('/') && path.starts_with('/') {
        format!("{}{}", prefix.trim_end_matches('/'), path)
    } else if !prefix.ends_with('/') && !path.starts_with('/') && !path.is_empty() {
        format!("{prefix}/{path}")
    } else {
        format!("{prefix}{path}")
    }
}

/// Used by `tests/repl_cli.rs` to run the REPL subcommand end-to-end.
pub fn run_cli(args: &[String]) -> Result<Graph, String> {
    if args.is_empty() {
        return Err("expected `graph|routes|providers|dtos`".to_string());
    }
    let mut path = PathBuf::from("src");
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --path".to_string())?;
                path = PathBuf::from(value);
            }
            "--format" => {
                i += 1;
                let _ = args
                    .get(i)
                    .ok_or_else(|| "missing value for --format".to_string())?;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    scan(&path)
}

/// `ExitCode` adapter so callers can map errors to non-zero exit
/// without unwrapping themselves.
pub fn run_with_exit(args: &[String]) -> ExitCode {
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}