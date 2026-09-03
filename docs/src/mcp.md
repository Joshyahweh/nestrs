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
| **Introspection** (source + live registries) | `list_modules`, `get_module`, `list_controllers`, `get_controller`, `list_providers`, `list_routes`, `get_route`, `list_dtos`, `get_dto`, `list_schedules`, `list_event_handlers`, `list_queue_processors` | nothing |
| **Live runtime** | `get_app_health`, `get_app_routes`, `get_app_providers` | nestrs app with the `admin` feature on |
| **Scaffolding** | `new_project`, `create_module`, `create_resource`, `create_dto`, `generate_crud` | write access to a target directory |
| **Docs search** | `search_docs`, `get_changelog`, `get_doc` | nothing (reads local repo files) |

Introspection reads the workspace's `src/` tree via `syn` (mirroring
the attribute shapes from `nestrs-macros`) and fuses that with
whatever the `RouteRegistry` and `ProviderRegistry` already hold.

## Install

```bash
# stdio only (default — sufficient for Claude Code, Cursor, VS Code, Codex)
cargo install nestrs-mcp

# With Streamable HTTP transport (networked / hosted use)
cargo install nestrs-mcp --features http

# With the admin-port client (talk to a running nestrs app's __nestrs/* endpoints)
cargo install nestrs-mcp --features admin
```

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
the MCP servers panel) and the `nestrs` tools (`list_modules`,
`get_app_health`, `create_resource`, `search_docs`, …) appear in the
model's tool list.

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

OAuth PKCE, a first-class bearer-token middleware, and per-tool
authorization are tracked as follow-ups; the live admin port on the
nestrs **app** side already supports bearer auth (see
`nestrs::admin::AdminOptions { token: ... }`).

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
servers panel). The nestrs tools appear in the model's tool list and
the model calls them automatically.

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

## Tool error conventions

- **Tool-level failure** (operation ran but failed): the tool returns
  a `CallToolResult::error` so the model can see the message and
  recover. Examples: file not found, parse error, app not reachable.
- **Protocol-level failure** (bad params, server can't process): the
  tool returns `Err(McpError::invalid_params(...))`.

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
  (`IsString`, `IsEmail`, `IsNotEmpty`, `IsUUID`, `MinLength`, `MaxLength`,
  `Min`, `Max`, `IsUrl`, `ValidateNested`, etc.)
- `#[ws_gateway(path = "/ws")]`, `#[ws_routes]`, `#[micro_routes]`,
  `#[event_routes]`, `#[schedule_routes]`

The parser is **strictly additive**: missing or unknown attributes are
reported as "unrecognized attr" but do not fail the parse. New macros
added to `nestrs-macros` will show up as unrecognized in `nestrs-mcp`
until the parser is updated — that is the intended maintenance
surface.

## See also

- [CLI (nestrs-scaffold)](cli.md) — `nestrs-cli new`, `nestrs-cli generate resource`
- [Production runbook](production.md) — observability and metrics
- [OpenAPI & HTTP](openapi-http.md) — schema generation
- [Custom decorators](custom-decorators.md) — building your own
  attribute-style macros
