mod resource_templates;

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy)]
enum Style {
    Nest,
    Rust,
}

#[derive(Clone, Copy)]
enum Transport {
    Rest,
    Graphql,
    Ws,
    Grpc,
    Microservice,
}

#[derive(Clone, Copy)]
struct CliOptions {
    dry_run: bool,
    force: bool,
    quiet: bool,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return print_help();
    }

    match args[0].as_str() {
        "g" | "generate" => generate(&args[1..]),
        "new" => create_new_project(&args[1..]),
        "--help" | "-h" | "help" => print_help(),
        other => Err(format!("unknown command `{other}`")),
    }
}

fn print_help() -> Result<(), String> {
    println!("nestrs CLI");
    println!();
    println!("Usage:");
    println!("  nestrs new <name> [--no-git] [--strict] [--package-manager cargo]");
    println!("  nestrs g|generate <resource|resources|service|controller|module|dto|guard|pipe|filter|interceptor|strategy|resolver|gateway|microservice|transport> <name> [--style nest|rust] [--path <dir>] [--dry-run] [--force] [--quiet]");
    println!("  nestrs g <res|s|co|mo|dto|gu|pi|fi|in|st|r|ga|ms|tr> <name> [--style nest|rust] [--path <dir>] [--dry-run] [--force] [--quiet]");
    println!("  nestrs g resource <name> [--transport rest|graphql|ws|grpc|microservice] [--style nest|rust] [--path <dir>] [--no-interactive] [--dry-run] [--force] [--quiet]");
    Ok(())
}

fn create_new_project(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("expected `nestrs new <name> ...`".to_string());
    }

    let name = args[0].clone();
    let mut no_git = false;
    let mut strict = false;
    let mut package_manager = String::from("cargo");

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--no-git" => no_git = true,
            "--strict" => strict = true,
            "--package-manager" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --package-manager".to_string())?;
                package_manager = value.clone();
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }

    if package_manager != "cargo" {
        return Err(format!(
            "unsupported package manager `{package_manager}`; only `cargo` is currently supported"
        ));
    }

    let root = PathBuf::from(&name);
    if root.exists() {
        return Err(format!("target path already exists: {}", root.display()));
    }

    fs::create_dir_all(root.join("src")).map_err(|e| e.to_string())?;

    let cargo_toml = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nnestrs = \"0.1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\n\n[profile.release]\nopt-level = 3\nlto = \"thin\"\ncodegen-units = 1\nstrip = \"symbols\"\npanic = \"abort\"\n"
    );
    fs::write(root.join("Cargo.toml"), cargo_toml).map_err(|e| e.to_string())?;

    let strict_attr = if strict {
        "#[serde(deny_unknown_fields)]\n"
    } else {
        ""
    };
    let main_rs = format!(
        "use nestrs::prelude::*;\n\n{strict_attr}#[dto]\npub struct PingDto {{\n    #[IsString]\n    pub message: String,\n}}\n\n#[controller(prefix = \"/\")]\npub struct AppController;\n\nimpl AppController {{\n    #[get(\"/\")]\n    pub async fn root() -> &'static str {{\n        \"Hello from nestrs\"\n    }}\n}}\n\n#[derive(Default)]\n#[injectable]\npub struct AppService;\n\nimpl_routes!(AppController, state AppService => [\n    GET \"/\" with () => AppController::root,\n]);\n\n#[module(\n    controllers = [AppController],\n    providers = [AppService],\n)]\npub struct AppModule;\n\n#[tokio::main]\nasync fn main() {{\n    let port = std::env::var(\"PORT\")\n        .ok()\n        .and_then(|v| v.parse::<u16>().ok())\n        .unwrap_or(3000);\n\n    NestFactory::create::<AppModule>()\n        .set_global_prefix(\"api\")\n        .use_request_id()\n        .use_request_tracing(RequestTracingOptions::builder().skip_paths([\"/metrics\"]))\n        .enable_metrics(\"/metrics\")\n        .enable_health_check(\"/health\")\n        .enable_production_errors_from_env()\n        .listen_graceful(port)\n        .await;\n}}\n"
    );
    fs::write(root.join("src/main.rs"), main_rs).map_err(|e| e.to_string())?;

    let env_example =
        "PORT=3000\nNESTRS_ENV=development\nRUST_LOG=info\nDATABASE_URL=file:./dev.db\n";
    fs::write(root.join(".env.example"), env_example).map_err(|e| e.to_string())?;
    fs::write(root.join(".env"), env_example).map_err(|e| e.to_string())?;
    let readme = format!(
        "# {name}\n\nGenerated with `nestrs new`.\n\n## Development\n\n```bash\ncargo run\n```\n\nServer defaults:\n- App: `http://127.0.0.1:3000/api`\n- Health: `http://127.0.0.1:3000/health`\n- Metrics: `http://127.0.0.1:3000/metrics`\n\n## Production profile\n\n```bash\nNESTRS_ENV=production RUST_LOG=info cargo run --release\n```\n\n## Docker\n\n```bash\ndocker build -t {name}:latest .\ndocker run -p 3000:3000 --env NESTRS_ENV=production {name}:latest\n```\n\n## Operations notes\n\n- `enable_production_errors_from_env()` sanitizes 5xx responses in production.\n- Configure CORS/security headers/rate limits per deployment needs.\n- Run DB migrations/seeds before release rollout when using a real database.\n"
    );
    fs::write(root.join("README.md"), readme).map_err(|e| e.to_string())?;
    let dockerfile = format!(
        "FROM rust:1.75 AS builder\nWORKDIR /app\nCOPY Cargo.toml Cargo.lock* ./\nCOPY src ./src\nRUN cargo build --release\n\nFROM debian:bookworm-slim\nRUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*\nCOPY --from=builder /app/target/release/{name} /usr/local/bin/{name}\nENV NESTRS_ENV=production\nENV PORT=3000\nEXPOSE 3000\nHEALTHCHECK CMD curl -f http://localhost:3000/health || exit 1\nCMD [\"/usr/local/bin/{name}\"]\n"
    );
    fs::write(root.join("Dockerfile"), dockerfile).map_err(|e| e.to_string())?;
    fs::write(root.join(".gitignore"), "/target\n.env\n").map_err(|e| e.to_string())?;

    if !no_git {
        let _ = Command::new("git").arg("init").current_dir(&root).status();
    }

    println!("created project {}", root.display());
    Ok(())
}

fn generate(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("expected `nestrs g <kind> <name> ...`".to_string());
    }

    let kind = args[0].as_str();
    let name = args[1].as_str();
    let mut style = Style::Nest;
    let mut transport = None::<Transport>;
    let mut base_path = PathBuf::from("src");
    let mut no_interactive = false;
    let mut dry_run = false;
    let mut force = false;
    let mut quiet = false;

    let mut i = 2usize;
    while i < args.len() {
        match args[i].as_str() {
            "--style" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --style".to_string())?;
                style = match value.as_str() {
                    "nest" => Style::Nest,
                    "rust" => Style::Rust,
                    other => return Err(format!("unknown style `{other}`")),
                };
            }
            "--transport" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --transport".to_string())?;
                transport = Some(match value.as_str() {
                    "rest" => Transport::Rest,
                    "graphql" => Transport::Graphql,
                    "ws" => Transport::Ws,
                    "grpc" => Transport::Grpc,
                    "microservice" => Transport::Microservice,
                    other => return Err(format!("unknown transport `{other}`")),
                });
            }
            "--path" => {
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| "missing value for --path".to_string())?;
                base_path = PathBuf::from(value);
            }
            "--no-interactive" => {
                no_interactive = true;
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--force" => {
                force = true;
            }
            "--quiet" => {
                quiet = true;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
        i += 1;
    }

    let opts = CliOptions {
        dry_run,
        force,
        quiet,
    };

    match canonical_kind(kind) {
        "service" => generate_unit(name, "service", style, &base_path, opts),
        "controller" => generate_unit(name, "controller", style, &base_path, opts),
        "module" => generate_unit(name, "module", style, &base_path, opts),
        "dto" => generate_unit(name, "dto", style, &base_path, opts),
        "guard" => generate_unit(name, "guard", style, &base_path, opts),
        "pipe" => generate_unit(name, "pipe", style, &base_path, opts),
        "filter" => generate_unit(name, "filter", style, &base_path, opts),
        "interceptor" => generate_unit(name, "interceptor", style, &base_path, opts),
        "strategy" => generate_unit(name, "strategy", style, &base_path, opts),
        "resolver" => generate_unit(name, "resolver", style, &base_path, opts),
        "gateway" => generate_unit(name, "gateway", style, &base_path, opts),
        "microservice" => generate_unit(name, "microservice", style, &base_path, opts),
        "transport" => generate_unit(name, "transport", style, &base_path, opts),
        "resource" => {
            let resolved_transport = match transport {
                Some(value) => value,
                None => {
                    if io::stdin().is_terminal() && !no_interactive {
                        prompt_transport()?
                    } else {
                        Transport::Rest
                    }
                }
            };
            generate_resource(name, style, resolved_transport, &base_path, opts)
        }
        other => Err(format!(
            "unknown generator kind `{}`",
            if other.is_empty() { kind } else { other }
        )),
    }
}

fn canonical_kind(kind: &str) -> &str {
    match kind {
        // Nest-style short aliases
        "res" | "resources" => "resource",
        "s" => "service",
        "co" => "controller",
        "mo" => "module",
        "gu" => "guard",
        "pi" => "pipe",
        "fi" => "filter",
        "in" => "interceptor",
        "st" => "strategy",
        "r" => "resolver",
        "ga" => "gateway",
        "ms" => "microservice",
        "tr" => "transport",
        // Full names
        "resource" | "resources" | "service" | "controller" | "module" | "dto" | "guard" | "pipe" | "filter"
        | "interceptor" | "strategy" | "resolver" | "gateway" | "microservice" | "transport" => {
            kind
        }
        _ => "",
    }
}

fn prompt_transport() -> Result<Transport, String> {
    println!("? Which transport layer would you like to use?");
    println!("  1) REST API");
    println!("  2) GraphQL");
    println!("  3) WebSockets");
    println!("  4) gRPC");
    println!("  5) Microservice");
    print!("Select option [1]: ");
    io::stdout().flush().map_err(|e| e.to_string())?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| e.to_string())?;
    let selection = input.trim();
    let value = if selection.is_empty() { "1" } else { selection };

    match value {
        "1" => Ok(Transport::Rest),
        "2" => Ok(Transport::Graphql),
        "3" => Ok(Transport::Ws),
        "4" => Ok(Transport::Grpc),
        "5" => Ok(Transport::Microservice),
        other => Err(format!("invalid transport selection `{other}`")),
    }
}

fn generate_unit(
    name: &str,
    kind: &str,
    style: Style,
    base_path: &Path,
    opts: CliOptions,
) -> Result<(), String> {
    let snake = to_snake(name);
    let pascal = to_pascal(name);
    let filename = file_name(&snake, kind, style);
    let path = base_path.join(&filename);
    ensure_parent(&path)?;
    let content = template_for(kind, &snake, &pascal);
    write_if_absent(&path, &content, opts)?;
    if !opts.quiet {
        if opts.dry_run {
            println!("dry-run create {}", path.display());
        } else {
            println!("created {}", path.display());
        }
    }
    Ok(())
}

fn generate_resource(
    name: &str,
    style: Style,
    transport: Transport,
    base_path: &Path,
    opts: CliOptions,
) -> Result<(), String> {
    let snake = to_snake(name);
    let pascal = to_pascal(name);
    let feature_dir = base_path.join(&snake);
    if !opts.dry_run {
        fs::create_dir_all(&feature_dir).map_err(|e| e.to_string())?;
    }

    let module_path = feature_dir.join(file_name(&snake, "module", style));
    let service_path = feature_dir.join(file_name(&snake, "service", style));
    let controller_kind = match transport {
        Transport::Graphql => "resolver",
        Transport::Ws => "gateway",
        Transport::Grpc => "grpc",
        Transport::Microservice => "transport",
        Transport::Rest => "controller",
    };
    let controller_path = feature_dir.join(file_name(&snake, controller_kind, style));
    let create_dto_path = feature_dir.join(file_name(&format!("create_{snake}"), "dto", style));
    let update_dto_path = feature_dir.join(file_name(&format!("update_{snake}"), "dto", style));
    let mod_rs_path = feature_dir.join("mod.rs");

    let rt = match transport {
        Transport::Rest => resource_templates::ResourceTransport::Rest,
        Transport::Graphql => resource_templates::ResourceTransport::Graphql,
        Transport::Ws => resource_templates::ResourceTransport::Ws,
        Transport::Grpc => resource_templates::ResourceTransport::Grpc,
        Transport::Microservice => resource_templates::ResourceTransport::Microservice,
    };

    let service_body = resource_templates::crud_service(&pascal);
    let module_body = resource_templates::resource_module(rt, &pascal);
    let entry_body = match transport {
        Transport::Rest => resource_templates::rest_controller(&snake, &pascal),
        Transport::Graphql => resource_templates::graphql_resolver(&snake, &pascal),
        Transport::Ws => resource_templates::ws_gateway(&snake, &pascal),
        Transport::Grpc => resource_templates::microservice_transport(&snake, &pascal, true),
        Transport::Microservice => resource_templates::microservice_transport(&snake, &pascal, false),
    };

    write_if_absent(&service_path, &service_body, opts)?;
    write_if_absent(&module_path, &module_body, opts)?;
    write_if_absent(&controller_path, &entry_body, opts)?;
    write_if_absent(
        &create_dto_path,
        &resource_templates::create_dto(&pascal),
        opts,
    )?;
    write_if_absent(
        &update_dto_path,
        &resource_templates::update_dto(&pascal),
        opts,
    )?;
    write_if_absent(
        &mod_rs_path,
        &mod_file_template(&snake, controller_kind, style),
        opts,
    )?;

    if !opts.quiet {
        if opts.dry_run {
            println!("dry-run create resource in {}", feature_dir.display());
        } else {
            println!("created resource in {}", feature_dir.display());
        }
    }
    wire_parent_module(base_path, &snake, &pascal, opts)?;
    Ok(())
}

fn wire_parent_module(
    base_path: &Path,
    snake: &str,
    pascal: &str,
    opts: CliOptions,
) -> Result<(), String> {
    let candidates = [
        base_path.join("lib.rs"),
        base_path.join("main.rs"),
        base_path.join("mod.rs"),
    ];
    let target = candidates.iter().find(|p| p.exists()).cloned();
    let Some(target) = target else {
        print_add_hints(snake, pascal, opts.quiet);
        return Ok(());
    };

    let existing = fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let mod_decl = format!("pub mod {snake};");
    let use_decl = format!("use crate::{snake}::{pascal}Module;");

    let has_mod = existing.contains(&mod_decl);
    let has_use = existing.contains(&use_decl);
    if has_mod && has_use {
        return Ok(());
    }

    let can_edit_safely = target
        .file_name()
        .and_then(|f| f.to_str())
        .map(|f| matches!(f, "lib.rs" | "main.rs" | "mod.rs"))
        .unwrap_or(false);
    if !can_edit_safely {
        print_add_hints(snake, pascal, opts.quiet);
        return Ok(());
    }

    let mut updated = existing;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !has_mod {
        updated.push_str(&format!("{mod_decl}\n"));
    }
    if !has_use {
        updated.push_str(&format!("{use_decl}\n"));
    }

    if opts.dry_run {
        if !opts.quiet {
            println!("dry-run wire parent module {}", target.display());
            if !has_mod {
                println!("dry-run add line: {mod_decl}");
            }
            if !has_use {
                println!("dry-run add line: {use_decl}");
            }
        }
        return Ok(());
    }

    if target.exists() && !opts.force {
        fs::write(&target, updated).map_err(|e| e.to_string())?;
        if !opts.quiet {
            println!("updated {}", target.display());
        }
        return Ok(());
    }

    fs::write(&target, updated).map_err(|e| e.to_string())?;
    if !opts.quiet {
        println!("updated {}", target.display());
    }
    Ok(())
}

fn print_add_hints(snake: &str, pascal: &str, quiet: bool) {
    if quiet {
        return;
    }
    println!("// ADD: pub mod {snake};");
    println!("// ADD: use crate::{snake}::{pascal}Module;");
}

fn file_name(stem: &str, suffix: &str, style: Style) -> String {
    match style {
        Style::Nest => format!("{stem}.{suffix}.rs"),
        Style::Rust => format!("{stem}_{suffix}.rs"),
    }
}

fn to_snake(input: &str) -> String {
    input
        .replace('-', "_")
        .chars()
        .enumerate()
        .flat_map(|(i, ch)| {
            if ch.is_uppercase() {
                if i == 0 {
                    vec![ch.to_ascii_lowercase()]
                } else {
                    vec!['_', ch.to_ascii_lowercase()]
                }
            } else {
                vec![ch]
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn to_pascal(input: &str) -> String {
    input
        .replace('-', "_")
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!(
                    "{}{}",
                    first.to_ascii_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn write_if_absent(path: &Path, content: &str, opts: CliOptions) -> Result<(), String> {
    if opts.dry_run {
        if !opts.quiet {
            println!("dry-run file {}", path.display());
        }
        return Ok(());
    }
    ensure_parent(path)?;
    if path.exists() && !opts.force {
        return Err(format!("file already exists: {}", path.display()));
    }
    fs::write(path, content).map_err(|e| e.to_string())
}

fn template_for(kind: &str, snake: &str, pascal: &str) -> String {
    match kind {
        "service" => format!(
            "use nestrs::prelude::*;\n\n#[derive(Default)]\n#[injectable]\npub struct {pascal}Service;\n\nimpl {pascal}Service {{\n    pub fn name(&self) -> &'static str {{\n        \"{snake}\"\n    }}\n}}\n"
        ),
        "controller" => format!(
            "use nestrs::prelude::*;\n\n#[controller(prefix = \"/{snake}\")]\npub struct {pascal}Controller;\n"
        ),
        "module" => format!(
            "use nestrs::prelude::*;\n\n#[module(\n    controllers = [{pascal}Controller],\n    providers = [{pascal}Service],\n)]\npub struct {pascal}Module;\n"
        ),
        "dto" => dto_template(&format!("{pascal}Dto")),
        "guard" => format!(
            "use nestrs::prelude::*;\n\n/// Use in `impl_routes!`: `with ({pascal}Guard)` or `controller_guards({pascal}Guard)`.\n#[derive(Default)]\npub struct {pascal}Guard;\n\n#[async_trait]\nimpl CanActivate for {pascal}Guard {{\n    async fn can_activate(\n        &self,\n        _parts: &axum::http::request::Parts,\n    ) -> Result<(), GuardError> {{\n        Ok(())\n    }}\n}}\n"
        ),
        "pipe" => format!(
            "use nestrs::prelude::*;\n\n/// Customize `Input` / `Output` / `Error` for your use case.\n#[derive(Default)]\npub struct {pascal}Pipe;\n\n#[async_trait]\nimpl PipeTransform<String> for {pascal}Pipe {{\n    type Output = String;\n    type Error = HttpException;\n\n    async fn transform(&self, value: String) -> Result<Self::Output, Self::Error> {{\n        Ok(value)\n    }}\n}}\n"
        ),
        "filter" => format!(
            "use nestrs::prelude::*;\n\n#[derive(Default)]\npub struct {pascal}Filter;\n\n#[async_trait]\nimpl ExceptionFilter for {pascal}Filter {{\n    async fn catch(&self, ex: HttpException) -> axum::response::Response {{\n        ex.into_response()\n    }}\n}}\n"
        ),
        "interceptor" => format!(
            "use nestrs::prelude::*;\n\n#[derive(Default)]\npub struct {pascal}Interceptor;\n\n#[async_trait]\nimpl Interceptor for {pascal}Interceptor {{\n    async fn intercept(\n        &self,\n        req: axum::extract::Request,\n        next: axum::middleware::Next,\n    ) -> axum::response::Response {{\n        next.run(req).await\n    }}\n}}\n"
        ),
        "strategy" => format!(
            "pub struct {pascal}Strategy;\n\nimpl {pascal}Strategy {{\n    pub fn name(&self) -> &'static str {{\n        \"{snake}\"\n    }}\n}}\n"
        ),
        "resolver" => format!(
            "pub struct {pascal}Resolver;\n"
        ),
        "gateway" => format!(
            "pub struct {pascal}Gateway;\n"
        ),
        "microservice" => format!(
            "use nestrs::prelude::*;\n\npub struct {pascal}Microservice;\n\nimpl {pascal}Microservice {{\n    pub fn transport(&self) -> &'static str {{\n        \"message-based\"\n    }}\n}}\n"
        ),
        "transport" => format!(
            "pub struct {pascal}Transport;\n"
        ),
        _ => String::from("// generated by nestrs-cli\n"),
    }
}

fn dto_template(type_name: &str) -> String {
    format!(
        "use nestrs::prelude::*;\n\n#[dto]\npub struct {type_name} {{\n    #[IsString]\n    #[Length(min = 1, max = 120)]\n    pub name: String,\n}}\n"
    )
}

fn mod_file_template(stem: &str, controller_kind: &str, style: Style) -> String {
    match style {
        Style::Nest => {
            let service_file = file_name(stem, "service", style);
            let module_file = file_name(stem, "module", style);
            let entry_file = file_name(stem, controller_kind, style);
            let create_dto_file = file_name(&format!("create_{stem}"), "dto", style);
            let update_dto_file = file_name(&format!("update_{stem}"), "dto", style);
            format!(
                "#[path = \"{module_file}\"]\npub mod {stem}_module;\n\
#[path = \"{entry_file}\"]\npub mod {stem}_{controller_kind};\n\
#[path = \"{service_file}\"]\npub mod {stem}_service;\n\
#[path = \"{create_dto_file}\"]\npub mod create_{stem}_dto;\n\
#[path = \"{update_dto_file}\"]\npub mod update_{stem}_dto;\n"
            )
        }
        Style::Rust => format!(
            "pub mod {stem}_module;\npub mod {stem}_{controller_kind};\npub mod {stem}_service;\npub mod create_{stem}_dto;\npub mod update_{stem}_dto;\n"
        ),
    }
}
