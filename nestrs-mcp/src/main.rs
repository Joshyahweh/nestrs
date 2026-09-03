//! `nestrs-mcp` — Model Context Protocol server for nestrs.
//!
//! Three modes:
//!
//! 1. **Server** (no subcommand, the default) — run as an MCP server over
//!    stdio (default) or HTTP, used by MCP-aware clients.
//! 2. **`init` / `setup`** — detect installed editors and write the
//!    `nestrs` MCP config into each one. Optional `--start-http-server`
//!    spawns the server in the background.

use std::io;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rmcp::service::serve_server;
#[cfg(feature = "http")]
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};

use nestrs_mcp::server::NestrsMcpServer;
#[cfg(feature = "http")]
use nestrs_mcp::wizard::DEFAULT_HTTP_ADDR;
use nestrs_mcp::wizard::{InitArgs, WizardTransport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

impl From<Transport> for WizardTransport {
    fn from(t: Transport) -> Self {
        match t {
            Transport::Stdio => WizardTransport::Stdio,
            Transport::Http => WizardTransport::Http,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "nestrs-mcp",
    version,
    about = "Model Context Protocol server for nestrs",
    long_about = "Run as an MCP server over stdio (default) or HTTP, or run `init`/`setup` to detect installed editors and write the right config files for the `nestrs` server."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Transport for the server mode (no subcommand). Ignored when a subcommand is given.
    #[arg(long, value_enum, default_value_t = Transport::Stdio, global = true)]
    transport: Transport,

    /// HTTP listen address for the server mode. Required when `--transport http`. Ignored otherwise.
    #[arg(long, global = true)]
    http_addr: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect installed editors and write the nestrs MCP config into each one.
    Init(InitCliArgs),
    /// Alias for `init` (matches the post-install UX verb used in the docs).
    Setup(InitCliArgs),
}

/// CLI-side mirror of `nestrs_mcp::wizard::InitArgs`. Kept separate so the
/// wizard module doesn't depend on `clap`.
#[derive(Debug, Args)]
struct InitCliArgs {
    /// Skip the multi-select editor prompt and use every detected editor.
    #[arg(long, short = 'y')]
    yes: bool,

    /// Print the plan without writing any files or spawning any servers.
    #[arg(long, conflicts_with = "yes")]
    no_interactive: bool,

    /// Default transport when running non-interactively or on first run.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    transport: Transport,

    /// Address the HTTP server should bind to (passed through to the spawn).
    #[arg(long)]
    http_addr: Option<String>,

    /// After writing configs with transport=http, spawn the server in the background
    /// and print its PID. Has no effect when transport=stdio.
    #[arg(long)]
    start_http_server: bool,
}

impl From<InitCliArgs> for InitArgs {
    fn from(a: InitCliArgs) -> Self {
        Self {
            yes: a.yes,
            no_interactive: a.no_interactive,
            transport: a.transport.into(),
            http_addr: a.http_addr,
            start_http_server: a.start_http_server,
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init(args) | Command::Setup(args)) => {
            if let Err(e) = nestrs_mcp::wizard::run(args.into()) {
                // The "multiple editors detected" hint is an ambiguous
                // input case (exit 2) so CI scripts can branch on it
                // distinct from a generic error (exit 1).
                let msg = format!("{e:?}");
                eprintln!("Error: {msg}");
                if msg.contains("multiple editors detected") {
                    std::process::exit(2);
                }
                std::process::exit(1);
            }
            Ok(())
        }
        None => run_server(cli.transport, cli.http_addr.as_deref()).await,
    }
}

async fn run_server(transport: Transport, http_addr: Option<&str>) -> io::Result<()> {
    match transport {
        Transport::Stdio => {
            let server = NestrsMcpServer::default();
            let transport = rmcp::transport::stdio();
            let running = serve_server(server, transport)
                .await
                .map_err(|e| io::Error::other(format!("stdio init: {e}")))?;
            running
                .waiting()
                .await
                .map_err(|e| io::Error::other(format!("stdio run: {e}")))?;
        }
        Transport::Http => {
            #[cfg(not(feature = "http"))]
            {
                let _ = http_addr;
                eprintln!(
                    "HTTP transport requested but the `http` feature is not enabled. \
                     Rebuild with `--features http`."
                );
                std::process::exit(2);
            }
            #[cfg(feature = "http")]
            {
                let default_addr = DEFAULT_HTTP_ADDR.to_string();
                let addr = http_addr
                    .unwrap_or(&default_addr)
                    .parse::<std::net::SocketAddr>()
                    .map_err(|e| {
                        io::Error::new(io::ErrorKind::InvalidInput, format!("bad addr: {e}"))
                    })?;
                let socket = tokio::net::TcpListener::bind(addr).await?;
                let service: StreamableHttpService<NestrsMcpServer, LocalSessionManager> =
                    StreamableHttpService::new(
                        || Ok(NestrsMcpServer::default()),
                        Default::default(),
                        StreamableHttpServerConfig::default(),
                    );
                let router = axum::Router::new().nest_service("/mcp", service);
                axum::serve(socket, router)
                    .await
                    .map_err(|e| io::Error::other(format!("http run: {e}")))?;
            }
        }
    }
    Ok(())
}

/// Tiny helper to keep `from_str` paths in the binary file when the
/// `http` feature is off.
#[allow(dead_code)]
fn _force_use_fromstr() {
    let _ = std::net::SocketAddr::from_str("127.0.0.1:0").unwrap();
}
