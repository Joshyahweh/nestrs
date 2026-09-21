# Model Context Protocol (MCP) server

`nestrs-mcp` is a Model Context Protocol server for nestrs. It exposes
your project's structure, live runtime state, and a set of scaffolding
actions to any MCP-aware client — Claude Code, Cursor, VS Code, Codex
CLI, or anything that speaks the protocol.

Once installed, the model can ask "what routes does this app define?",
"is the app healthy right now?", or "scaffold a `User` resource with
name and email fields" — and get a structured answer without re-parsing
the source tree on every turn.

## What it exposes

| Surface | Example tools | Requires |
|---|---|---|
| **Introspection** (source parser, read-only) | `list_modules`, `get_module`, `list_controllers`, `get_controller`, `list_providers`, `get_provider`, `list_routes`, `get_route`, `list_dtos`, `get_dto`, `list_schedules`, `list_event_handlers`, `list_queue_processors` | nothing |
| **Live runtime** | `get_app_health`, `get_app_routes`, `get_app_providers` | nestrs app with the `admin` feature on |
| **Scaffolding** | `new_project`, `create_module`, `create_resource`, `create_dto`, `generate_crud` | write access to a target directory |
| **Docs search** | `search_docs`, `get_changelog`, `get_doc` | nothing (reads local repo files) |

Introspection reads the workspace's `src/` tree via `syn` (mirroring
the attribute shapes from `nestrs-macros`) — no running app needed.
The live-runtime tools are separate: they query a running app's admin
port over HTTP (see [Live runtime](#talking-to-a-running-nestrs-app-live-runtime)).

## Server architecture

The crate ships one binary and one library, with a deliberate split
between them:

- `NestrsMcpServer` (`nestrs_mcp::server`) is the wrapper the binary
  serves. It carries every protocol surface except tools — prompts,
  resources, resource templates, completion, subscriptions, cache
  hints, elicitation, tasks, and the per-tool-call authorization
  pipeline.
- The **tools** live on four area handlers, each a complete rmcp
  `ServerHandler` in its own right:
  `nestrs_mcp::tools::introspection::IntrospectionTools` (13 tools),
  `nestrs_mcp::tools::runtime::RuntimeTools` (3 tools),
  `nestrs_mcp::tools::scaffold::ScaffoldTools` (5 tools), and
  `nestrs_mcp::tools::docs::DocsTools` (3 tools).

> The stock `nestrs-mcp` binary advertises every area tool. rmcp's
> `#[tool_router]` type is invariant in `Self`, so the sub-routers cannot
> be merged into `NestrsMcpServer`'s router — `list_tools` / `get_tool` /
> `call_tool` dispatch to the four area handlers instead. Embedding a
> single area handler is still supported when you want a narrower surface.

### Serving a single area yourself

Each area handler is a standalone `ServerHandler`, so a three-line
`main.rs` serves any subset without spawning a subprocess:

```rust
use nestrs_mcp::tools::introspection::IntrospectionTools;
use rmcp::service::serve_server;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let running = serve_server(IntrospectionTools, stdio())
        .await
        .map_err(|e| std::io::Error::other(format!("stdio init: {e}")))?;
    running
        .waiting()
        .await
        .map_err(|e| std::io::Error::other(format!("stdio run: {e}")))?;
    Ok(())
}
```

```toml
# Cargo.toml — note the direct rmcp dependency; it is not re-exported
[dependencies]
nestrs-mcp = "1.4.0"
rmcp = { version = "3.1", default-features = false, features = ["server", "transport-io"] }
tokio = { version = "1", features = ["full"] }
```

Swap `IntrospectionTools` for `RuntimeTools`, `ScaffoldTools`, or
`DocsTools` (or serve several in parallel on separate transports). The
wizard's client configs point at whatever binary name you publish.

## Install

```bash
# stdio only (default — sufficient for Claude Code, Cursor, VS Code, Codex)
cargo install nestrs-mcp

# With Streamable HTTP transport (networked / hosted use)
cargo install nestrs-mcp --features http

# With the nestrs::admin re-exports (AdminHandle / AdminOptions) for embedders
cargo install nestrs-mcp --features admin
```

> **Note:** the admin-port tools (`get_app_health`, `get_app_routes`,
> `get_app_providers`) and the `AdminClient` behind them are compiled
> in unconditionally — no feature needed on `nestrs-mcp`. The `admin`
> feature only re-exports `nestrs::admin::{AdminHandle, AdminOptions}`.
> The feature that matters for the runtime tools is `admin` on the
> **`nestrs` crate of the target app** (see [Live runtime](#talking-to-a-running-nestrs-app-live-runtime)).

As a library dependency, the same features apply. The full feature
list is `stdio` (default, marker only), `http`, `admin`, `authz`,
`authz-row-level`, `elicitation`.

## Setup wizard

`cargo install` is half the story — the client still needs to know
about the server. The `init` subcommand (alias: `setup`) detects
installed editors by checking the well-known config paths, asks
which ones to configure, and writes the right MCP server entry into
each one — idempotently preserving everything else.

```bash
# Interactive: detects installed editors, lets you toggle, picks a transport.
nestrs-mcp init

# Same thing, no questions:
nestrs-mcp init --yes

# Dry-run / scripted use: print what WOULD have been written, write nothing.
nestrs-mcp init --no-interactive

# Use HTTP transport and spawn the server in the background after writing configs.
nestrs-mcp init --yes --transport http --start-http-server
```

| Flag | Effect |
|---|---|
| `--yes`, `-y` | Skip the multi-select editor prompt; use every detected editor. |
| `--no-interactive` | Print the plan without writing any files or spawning any servers. Distinct from `--yes`: lets CI scripts preview before applying. |
| `--transport <stdio\|http>` | Which transport to write. Default `stdio`. |
| `--http-addr <addr>` | HTTP listen address. Default `127.0.0.1:7777`. |
| `--start-http-server` | After writing configs with `--transport http`, spawn the server in the background and print its PID. No effect with `--transport stdio`. |

Detection rules: an editor is "detected" if its config file **or** its
parent directory exists. So a fresh checkout with `.vscode/` but no
`mcp.json` still gets offered the option to create the file.

Merge behavior: all four formats (`mcpServers` for Claude Code / Cursor,
`servers` for VS Code, `[mcp_servers]` for Codex) are merged round-trip —
the wizard preserves every unrelated key and every other server entry.
A second run is a no-op (the file is byte-identical, nothing is rewritten).

After the wizard finishes, restart your editor (or click **Refresh** in
the MCP servers panel) and the `nestrs` tools become callable —
provided the binary your config points at actually serves them (see
[Server architecture](#server-architecture)).

> Codex's `config.toml` may show unrelated diff hunks on the first run —
> that's `toml::to_string_pretty` re-formatting the existing file. Commit
> the new file once and you're set; subsequent runs produce no diff.

## Run

```bash
# stdio (default) — the client spawns the binary and speaks JSON-RPC
# over its stdin/stdout.
nestrs-mcp

# Streamable HTTP — the binary listens on <addr> and serves the
# MCP endpoint at /mcp. Requires `--features http` at install time.
nestrs-mcp --transport http --http-addr 127.0.0.1:7777
```

The HTTP transport is a `StreamableHttpService` mounted on an
`axum::Router` at `/mcp`.

## Security

**The HTTP transport has no built-in auth.** `nestrs-mcp --transport
http` binds to the address you give it and serves `/mcp` to anyone who
can reach it. Every tool — including the destructive scaffolding
actions (`new_project`, `create_module`, `create_resource`,
`create_dto`, `generate_crud`) — is reachable. This is intentional
for v1 (the recommended path is `stdio` with a local subprocess), but
it is a real gap before exposing `:7777` to anything beyond localhost.

**Before you expose the HTTP transport to a non-loopback address:**

1. Put a reverse proxy in front of it that terminates TLS **and**
   enforces auth. Any of these work and are well-trodden:
   - Caddy: `reverse_proxy 127.0.0.1:7777 { basicauth { ... } }`
   - nginx: `auth_basic "nestrs-mcp"; auth_basic_user_file ...;`
   - Cloudflare Tunnel + Cloudflare Access (zero-trust JWTs in front
     of the local listener).
2. Bind the listener to `127.0.0.1`, never `0.0.0.0`, so the proxy is
   the only way in.
3. Set `--http-addr 127.0.0.1:<port>` explicitly; the default is
   already loopback, but spell it out so a later refactor can't widen
   the bind by accident.

OAuth PKCE and a first-class bearer-token middleware are tracked as
follow-ups; the live admin port on the nestrs **app** side already
supports bearer auth (see `nestrs::admin::AdminOptions { token: ... }`),
and per-tool-call authorization on the MCP side itself is available
today via the `authz` feature (see
[Per-tool-call authorization](#per-tool-call-authorization-authz)).

## Connect from a client

`nestrs-mcp` speaks the standard Model Context Protocol. The two
patterns are **stdio** (the client spawns the binary as a subprocess)
and **Streamable HTTP** (the client connects to a running server).

### stdio (local, recommended)

The client launches `nestrs-mcp` on demand and pipes JSON-RPC through
its stdin/stdout — no ports, no auth, no leftover processes.

**Claude Code** — `.mcp.json` in your project root (or
`~/.claude.json` for a global install):

```json
{
  "mcpServers": {
    "nestrs": {
      "command": "nestrs-mcp",
      "args": []
    }
  }
}
```

**Cursor** — `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "nestrs": {
      "command": "nestrs-mcp",
      "args": []
    }
  }
}
```

**VS Code (GitHub Copilot Chat)** — `.vscode/mcp.json` in your
workspace:

```json
{
  "servers": {
    "nestrs": {
      "type": "stdio",
      "command": "nestrs-mcp",
      "args": []
    }
  }
}
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.nestrs]
command = "nestrs-mcp"
args = []
```

After saving, restart the client (or click "Refresh" in the MCP
servers panel). The nestrs tools become callable by the model —
provided the binary your config points at actually serves them (see
[Server architecture](#server-architecture)).

### Streamable HTTP (networked / hosted)

Useful when the binary runs on a host the client can't shell into, or
when several clients should share one server.

Start the server (it stays in the foreground; run it under your
process supervisor of choice):

```bash
nestrs-mcp --transport http --http-addr 127.0.0.1:7777
```

Then point the client at `http://<host>:7777/mcp`:

**Claude Code** — `.mcp.json`:

```json
{
  "mcpServers": {
    "nestrs": {
      "url": "http://127.0.0.1:7777/mcp"
    }
  }
}
```

**Cursor** — `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "nestrs": {
      "url": "http://127.0.0.1:7777/mcp"
    }
  }
}
```

**VS Code (GitHub Copilot Chat)** — `.vscode/mcp.json`:

```json
{
  "servers": {
    "nestrs": {
      "type": "http",
      "url": "http://127.0.0.1:7777/mcp"
    }
  }
}
```

**Codex CLI** — `~/.codex/config.toml`:

```toml
[mcp_servers.nestrs]
url = "http://127.0.0.1:7777/mcp"
```

### Verifying the connection

From the shell, a quick sanity check that the HTTP transport is
alive:

```bash
curl -sS -X POST http://127.0.0.1:7777/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.1"}}}'
```

A `200 OK` with an `mcp-session-id` header and a JSON `result` block
means the handshake succeeded and the client can call tools.

## Talking to a running nestrs app (live runtime)

The `get_app_health`, `get_app_routes`, and `get_app_providers` tools
hit a localhost-only sidecar exposed by
`NestApplication::use_admin(AdminOptions)` in the `nestrs` crate's
`admin` feature. To enable it, the app's `Cargo.toml` needs:

```toml
nestrs = { path = "../nestrs", features = ["admin"] }
```

and the binary needs:

```rust
use nestrs::admin::AdminOptions;

let app = NestFactory::create::<AppModule>().enable_health_check("/live");
let admin = app.use_admin(AdminOptions {
    addr: "127.0.0.1:7777".parse()?,
    token: Some(std::env::var("NESTRS_ADMIN_TOKEN")?),
});
tokio::spawn(async move { let _ = admin.serve().await; });
```

The sidecar exposes:

- `GET /__nestrs/health` — `{ status, uptime_ms, version }`
- `GET /__nestrs/providers` — `Vec<{ type_name, scope }>`
- `GET /__nestrs/routes` — `Vec<RouteInfo>` from the `RouteRegistry`
- `GET /__nestrs/openapi.json` — proxy of the OpenAPI doc

If a token is configured, requests must carry
`Authorization: Bearer <token>`. Without a token the listener refuses
to bind to anything but `127.0.0.1` and responds `401` to all routes.

The MCP `get_app_health` / `get_app_routes` / `get_app_providers`
tools take `base_url` + optional `token` per call, so the model can
target a running app on the user's machine without restarting the
server.

## Protocol surfaces beyond tools

The **`NestrsMcpServer`** wrapper (through its **`McpSurfaces`** bundle) implements the MCP surfaces that aren't tools: prompts, resources, resource templates, argument completion, resource subscriptions, and cache hints. Build an **`McpSurfaces`** value, register what you need, and attach it with **`.with_surfaces(...)`**:

```rust
use nestrs_mcp::{user_text, CacheHints, McpSurfaces};
use rmcp::model::{CacheScope, PromptArgument, Resource, ResourceContents, ResourceTemplate};

let surfaces = McpSurfaces::new()
    .register_prompt(
        "review",
        Some("Code review prompt".into()),
        vec![PromptArgument::new("topic").with_required(true)],
        |args| Ok(vec![user_text("Review this code")]),
    )
    .register_resource(
        Resource::new("nestrs://health", "health").with_mime_type("text/plain"),
        |uri| Ok(vec![ResourceContents::text("ok", uri)]),
    )
    .register_resource_template(
        ResourceTemplate::new("nestrs://docs/{name}", "doc_tmpl"),
        |uri| Ok(vec![ResourceContents::text("doc body", uri)]),
    )
    .register_complete(|partial, _req| Ok(vec![format!("{partial}-alpha")]))
    .register_subscribable_resource("nestrs://health")
    .with_cache_hints(CacheHints::new(60_000, CacheScope::Public));

let server = nestrs_mcp::server::NestrsMcpServer::new().with_surfaces(surfaces);
```

Matching rules worth knowing:

- **`resources/read`** checks the static resource map first — a static resource always wins over a template matching the same URI.
- **Template matching** is a static-prefix match (everything before the first `{`, trailing `/` trimmed) — enough for the common `nestrs://docs/{name}` shape.
- **`resources.subscribe`** is advertised only when at least one URI is marked subscribable; protocol ≥ 2026-07-28 peers use the **`subscriptions/listen`** flow, older peers keep the legacy subscribe methods.
- **Cache hints** (SEP-2549) travel as top-level `ttlMs` / `cacheScope` fields, suppressed for peers on older protocol versions. `CacheScope::Private` restricts caching to the requesting user's client.

## Elicitation: asking the user mid-flight

Two mechanisms, both opt-in:

**MRTR for tools (SEP-2322)** — no Cargo feature needed. A tool needing more input returns **`CallToolResponse::InputRequired`** instead of an error, via the **`elicit_input`** helper; on retry the client echoes the answer and **`input_responses`** reads it. The `request_state` argument (`Option<String>`) carries opaque server-side state across rounds.

**Server→client elicitation (SEP-1034)** — behind the **`elicitation`** feature (adds rmcp's elicitation support, including URL-based elicitations). Register one handler on the surfaces bundle (**`.register_elicitation(...)`**), then call **`NestrsMcpServer::elicit`** from a tool body. It probes the client's capability first and returns a `Cancel` result (rather than an error) if the client didn't advertise elicitation.

## Long-running tasks (SEP-2663)

A tool that kicks off slow work can hand the client a task handle instead of blocking the `tools/call` response. The task store always exists; advertise the extension with **`.with_task_support()`** so clients know they can poll:

```rust
let server = nestrs_mcp::server::NestrsMcpServer::new().with_task_support();

// Inside a tool body — the future must be 'static (move owned data in):
let task = server.spawn_task(TaskOptions::default(), |ctx| {
    Box::pin(async move {
        // Cooperative cancellation + mid-flight input, both on `ctx`:
        //   tokio::select! { _ = ctx.cancelled() => Err(TaskExit::Cancelled), ... }
        //   let answer = ctx.request_input("input", request).await?;
        Ok(CallToolResult::success(vec![ContentBlock::text("done")]))
    })
});
// Return `CallToolResponse::Task(CreateTaskResult::new(task))` from the tool.
```

- **`tasks/get` / `tasks/update` / `tasks/cancel`** are rejected with `-32601` unless `.with_task_support()` advertised the `io.modelcontextprotocol/tasks` extension.
- An operation can request input mid-flight via **`ctx.request_input(key, request)`** — the same MRTR loop as tools, applied to tasks.
- Cancellation is cooperative: `tasks/cancel` acknowledges immediately; the operation observes **`ctx.cancelled()`** and exits with `TaskExit::Cancelled`.

## Per-tool-call authorization (`authz`)

Behind the **`authz`** feature, the wrapper's `ServerHandler::call_tool` override runs every tool call inside the same task-local scopes the HTTP, WebSocket, and GraphQL transports use:

```rust
let ctx = McpDataContext::new()
    .with_ability(ability)     // Arc<nestrs::Ability> — CASL-style rules
    .with_principal(principal) // Arc<nestrs::policies::Principal> — row-level predicates
    .with_pool(pool);          // Arc<sqlx::AnyPool> — per-tool-call transactions

let server = nestrs_mcp::server::NestrsMcpServer::new().with_data_context(ctx);
```

On every `tools/call` the override: opens a **`TransactionSlot`** on the configured pool (tool bodies read it via `current_mcp_transaction()`), installs the ability + principal (`current_mcp_ability()` / `current_mcp_principal()`), dispatches the tool, post-masks the response with **`nestrs::mask_value`** (the same masking walker HTTP/WS/GraphQL use), and commits or rolls back the transaction. Tool-error results are never masked (stripping fields would hide the diagnostic). **`authz-row-level`** additionally wires the row-level predicates. See [Authorization](authorization.md) for the model itself.

## Tool error conventions

Tool bodies return `Result<Json<...>, rmcp::ErrorData>`:

- **Operational failure** (operation ran but failed — file not found,
  parse error, app not reachable, scaffold failure): the tool returns
  `Err(ErrorData::internal_error(msg))`. The message is visible to the
  model, which can read it and recover or retry.
- **Bad parameters** (unknown module / controller / provider / DTO /
  route name): `Err(ErrorData::invalid_params("module `x` not found"))`.
- **Success**: a `Json<T>` payload — the typed value lands in
  `structuredContent` (the `#[tool]` macro stamps an `outputSchema`
  from the return type) plus a text fallback block for clients that
  only render text.

## Source parser

`nestrs-mcp` re-implements the attribute parser in
`introspection::source` using `syn` directly. It does not depend on
`nestrs-macros` (it is `proc-macro` only and would create a
build-time circular dep). The parser recognizes:

- `#[module(...)]` — `imports`, `controllers`, `providers`, `microservices`, `exports`, `re_exports`
- `#[controller("/path"[, version, host])]` — emits
  `__nestrs_prefix` / `__nestrs_version` / `__nestrs_host` const fns
- `#[routes(state, controller_guards)]` impls with their per-fn
  attributes: `#[get/post/put/patch/delete/options/head/all(...)]`,
  `#[ver(...)]`, `#[use_guards(...)]`, `#[use_interceptors(...)]`,
  `#[use_pipes(...)]`, `#[use_filters(...)]`, `#[set_metadata(...)]`,
  `#[roles(...)]`, `#[param::body/query/param/req/headers/ip]`,
  `#[openapi(...)]`
- `#[injectable(scope = "singleton|transient|request")]`
- `#[dto(...)]` and its field-attr translation table
  (`IsString`, `IsEmail`, `IsNotEmpty`, `IsUuid`, `MinLength`, `MaxLength`,
  `Min`, `Max`, `IsUrl`, `ValidateNested`, etc.)
- `#[ws_gateway(path = "/ws")]`, `#[ws_routes]`, `#[micro_routes]`,
  `#[event_routes]`, `#[schedule_routes]`

The parser is **strictly additive**: missing or unknown attributes are
reported as "unrecognized attr" but do not fail the parse. New macros
added to `nestrs-macros` will show up as unrecognized in `nestrs-mcp`
until the parser is updated — that is the intended maintenance
surface.

Scaffolding guards are name-only: module and DTO names must be valid
Rust idents, and project names must be valid crate names. There is no
workspace-containment check on `path`, so treat tool-supplied output
paths as trusted input.

## See also

- [CLI (nestrs-scaffold)](cli.md) — `nestrs-cli new`, `nestrs-cli generate resource`
- [Production runbook](production.md) — observability and metrics
- [OpenAPI & HTTP](openapi-http.md) — schema generation
- [Custom decorators](custom-decorators.md) — building your own
  attribute-style macros
- [Authorization](authorization.md) — the guards/policies/row-level
  model the MCP `authz` pipeline mirrors
