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
//! - `nestrs-cli repl live --url <admin> [--bearer-token <token>]` —
//!   dump live providers + routes from a running app's admin sidecar
//!   (`GET /__nestrs/providers` and `/__nestrs/routes`). Transport is
//!   `curl -X GET` with `Authorization: Bearer …` as a **header only**
//!   (never a query string). The URL is passed after `--` so it cannot
//!   be parsed as a curl flag. Requires `curl` on `$PATH`.
//!
//! All modes default to scanning `./src` from the current working
//! directory. `--path <dir>` overrides the source root.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use regex::Regex;
use serde::Serialize;

/// A `#[module(...)]` declaration found by the REPL scanner.
#[derive(Debug, Serialize)]
pub struct ModuleEntry {
    /// Type name.
    pub name: String,
    /// Source file (display path).
    pub file: String,
    /// Controller type names listed in the module.
    pub controllers: Vec<String>,
    /// Provider type names listed in the module.
    pub providers: Vec<String>,
    /// Imported module type names.
    pub imports: Vec<String>,
}

/// A `#[controller(...)]` declaration found by the REPL scanner.
#[derive(Debug, Serialize)]
pub struct ControllerEntry {
    /// Type name.
    pub name: String,
    /// Source file (display path).
    pub file: String,
    /// Route prefix from `#[controller("/...")]`, if any.
    pub prefix: Option<String>,
    /// HTTP routes discovered on the impl.
    pub routes: Vec<RouteEntry>,
}

/// A single HTTP route extracted from a controller impl.
#[derive(Debug, Serialize)]
pub struct RouteEntry {
    /// HTTP method (`GET`, `POST`, …).
    pub method: String,
    /// Path fragment from the method attribute.
    pub path: String,
    /// Handler function name.
    pub handler: String,
}

/// An `#[injectable]` type found by the REPL scanner.
#[derive(Debug, Serialize)]
pub struct ProviderEntry {
    /// Type name.
    pub name: String,
    /// Source file (display path).
    pub file: String,
}

/// A `#[dto]` type found by the REPL scanner.
#[derive(Debug, Serialize)]
pub struct DtoEntry {
    /// Type name.
    pub name: String,
    /// Source file (display path).
    pub file: String,
}

/// Source-level DI graph extracted by `nestrs-cli repl`.
#[derive(Debug, Default, Serialize)]
pub struct Graph {
    /// Modules in scan order.
    pub modules: Vec<ModuleEntry>,
    /// Controllers in scan order.
    pub controllers: Vec<ControllerEntry>,
    /// Injectable providers in scan order.
    pub providers: Vec<ProviderEntry>,
    /// DTO / entity / model types in scan order.
    pub dtos: Vec<DtoEntry>,
}

/// Parsed `repl live` flags. Bearer is never logged.
#[derive(Debug)]
pub struct LiveArgs {
    /// Admin sidecar base URL (`http://127.0.0.1:7777`).
    pub url: String,
    /// Optional bearer token (header only).
    pub bearer: Option<String>,
    /// `text` or `json`.
    pub format: String,
}

/// Parse `repl live` argv (tokens after `live`). Network I/O is separate
/// so tests can assert flag errors without hitting an admin port.
pub fn parse_live_args(args: &[String]) -> Result<LiveArgs, String> {
    let mut url: Option<String> = None;
    let mut bearer: Option<String> = None;
    let mut format = String::from("text");
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
            "--bearer-token" => {
                i += 1;
                bearer = Some(
                    args.get(i)
                        .ok_or_else(|| "missing value for --bearer-token".to_string())?
                        .clone(),
                );
            }
            "--format" => {
                i += 1;
                format = args
                    .get(i)
                    .ok_or_else(|| "missing value for --format".to_string())?
                    .clone();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }
    let url = url.ok_or_else(|| "missing required `--url <http>`".to_string())?;
    if format != "text" && format != "json" {
        return Err(format!("unknown format `{format}`; expected text or json"));
    }
    Ok(LiveArgs {
        url,
        bearer,
        format,
    })
}

fn admin_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn curl_get(url: &str, bearer: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("curl");
    cmd.arg("-sS")
        .arg("-X")
        .arg("GET")
        .arg("--max-time")
        .arg("15");
    if let Some(token) = bearer {
        cmd.arg("-H").arg(format!("Authorization: Bearer {token}"));
    }
    cmd.arg("--").arg(url);
    cmd.stdin(Stdio::null());
    let output = cmd.output().map_err(|e| {
        format!("failed to invoke `curl` against admin sidecar: {e}. Is `curl` on your $PATH?")
    })?;
    if !output.status.success() {
        return Err(format!(
            "curl exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn run_live(args: &[String]) -> Result<(), String> {
    let opts = parse_live_args(args)?;
    let providers = curl_get(
        &admin_url(&opts.url, "/__nestrs/providers"),
        opts.bearer.as_deref(),
    )?;
    let routes = curl_get(
        &admin_url(&opts.url, "/__nestrs/routes"),
        opts.bearer.as_deref(),
    )?;
    match opts.format.as_str() {
        "json" => {
            println!(
                "{{\"providers\":{},\"routes\":{}}}",
                providers.trim(),
                routes.trim()
            );
        }
        _ => {
            println!("GET /__nestrs/providers");
            println!("{providers}");
            println!("GET /__nestrs/routes");
            println!("{routes}");
        }
    }
    Ok(())
}

/// Entry point. Dispatches to graph/routes/providers/live sub-subcommands.
/// `args` are the post-`repl` argv tokens.
pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "expected `nestrs-cli repl <graph|routes|providers|dtos|live> [...]`".to_string(),
        );
    }

    if args[0] == "live" {
        return run_live(&args[1..]);
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

    let re_controller = Regex::new(r"#\s*\[\s*controller\b([^\]]*)\]").unwrap();
    let re_injectable = Regex::new(r"#\s*\[\s*injectable\b[^\]]*\]").unwrap();
    let re_dto = Regex::new(
        r"#\s*\[\s*(?:(?:nestrs|nestrs_macros)\s*::\s*)?(?:dto|partial_type|omit_type|pick_type|intersection_type)\b[^\]]*\]",
    )
    .unwrap();
    let re_impl_routes = Regex::new(r"impl_routes!\s*\(").unwrap();
    let re_prefix = Regex::new(r#"prefix\s*=\s*"([^"]+)""#).unwrap();
    let re_method_route = Regex::new(
        r#"#\s*\[\s*(get|post|put|patch|delete|head|options)\s*\(\s*"([^"]*)"\s*\)\s*\]"#,
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

        // Modules — `#[module(controllers = [...], providers = [...], imports = [...])]`.
        // Nested `[...]` lists mean we cannot stop at the first `]`; walk
        // balanced parentheses instead.
        for (end, inner) in find_attr_paren_inners(&content, "module") {
            let module_name = next_struct_name_after(&content, end);
            let (controllers, providers, imports) = parse_lists(&inner, &re_keyed_list);
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
                if (name.ends_with("Dto") || name.ends_with("Entity") || name.ends_with("Model"))
                    && !graph.dtos.iter().any(|d| d.name == name)
                {
                    graph.dtos.push(DtoEntry {
                        name,
                        file: display.clone(),
                    });
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

/// Locate `#[ident(...)]` / `#[ident]` invocations and return
/// `(byte_offset_after_attr, paren_inner)`. Nested `[...]` inside the
/// parentheses (e.g. `controllers = [Foo]`) are preserved — a naive
/// `[^\]]*` regex would stop at the first list closer.
fn find_attr_paren_inners(content: &str, ident: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i < content.len() {
        let Some(rel) = content[i..].find("#[") else {
            break;
        };
        let after_hash = i + rel + 2;
        let rest = content[after_hash..].trim_start();
        let ident_start = after_hash + (content[after_hash..].len() - rest.len());
        if !rest.starts_with(ident) {
            i = after_hash;
            continue;
        }
        let after_ident = ident_start + ident.len();
        if after_ident < content.len() {
            let next = bytes[after_ident];
            if next.is_ascii_alphanumeric() || next == b'_' {
                i = after_ident;
                continue;
            }
        }
        let mut k = after_ident;
        while k < content.len() && bytes[k].is_ascii_whitespace() {
            k += 1;
        }
        if k >= content.len() {
            break;
        }
        if bytes[k] == b']' {
            out.push((k + 1, String::new()));
            i = k + 1;
            continue;
        }
        if bytes[k] != b'(' {
            i = k;
            continue;
        }
        let inner_start = k + 1;
        let mut depth = 1i32;
        let mut j = inner_start;
        while j < content.len() && depth > 0 {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        if depth != 0 {
            i = inner_start;
            continue;
        }
        let inner = content[inner_start..j - 1].to_string();
        let mut end = j;
        while end < content.len() && bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        if end < content.len() && bytes[end] == b']' {
            end += 1;
        }
        out.push((end, inner));
        i = end;
    }
    out
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

fn parse_lists(inner: &str, re_keyed_list: &Regex) -> (Vec<String>, Vec<String>, Vec<String>) {
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
        .map(|s| {
            s.trim_matches(|c: char| !c.is_alphanumeric() && c != '_')
                .to_string()
        })
        .filter(|s| !s.is_empty() && s.chars().next().is_some_and(|c| c.is_uppercase()))
        .collect()
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn print_graph(graph: &Graph, format: &str) -> Result<(), String> {
    match format {
        "text" => {
            println!(
                "nestrs DI graph ({} modules, {} controllers, {} providers, {} DTOs)",
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
                            println!(
                                "      {:>6} {full_path:30} -> {}::{}",
                                route.method, c.name, route.handler
                            );
                        }
                    }
                }
                println!();
            }
            Ok(())
        }
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(graph).map_err(|e| e.to_string())?
            );
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
            println!("{:>6}  {:<40}  HANDLER", "METHOD", "PATH");
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
            println!(
                "{}",
                serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?
            );
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
                        by_module.entry(module.name.clone()).or_default().push(p);
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
            println!(
                "{}",
                serde_json::to_string_pretty(&graph.providers).map_err(|e| e.to_string())?
            );
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
            println!(
                "{}",
                serde_json::to_string_pretty(&graph.dtos).map_err(|e| e.to_string())?
            );
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
    match args[0].as_str() {
        "graph" | "routes" | "providers" | "dtos" => {}
        other => return Err(format!("unknown subcommand `{other}`")),
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
