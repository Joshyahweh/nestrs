# nestrs-mcp

A [Model Context Protocol](https://modelcontextprotocol.io/) server for
[nestrs](https://github.com/Joshyahweh/nestrs). It exposes nestrs projects to
MCP-aware clients (Claude Code, Cursor, VS Code, …) as a structured set of
tools so the model can ask "what routes does this app define?" instead of
re-parsing the macros on every turn.

## What it exposes

| Surface | Tools | Requires |
|---|---|---|
| **Introspection** (source + live registries) | `list_modules`, `get_module`, `list_controllers`, `get_controller`, `list_providers`, `get_provider`, `list_routes`, `get_route`, `list_dtos`, `get_dto`, `list_schedules`, `list_event_handlers`, `list_queue_processors` | nothing |
| **Live runtime** | `get_app_health`, `get_app_routes`, `get_app_providers` | nestrs app with the `admin` feature on |
| **Scaffolding** | `new_project`, `create_module`, `create_resource`, `create_dto`, `generate_crud` | write access to a target directory |
| **Docs search** | `search_docs`, `get_changelog`, `get_doc` | nothing (reads local repo files) |

Introspection reads the workspace's `src/` tree via `syn` (mirroring the
attribute shapes from `nestrs-macros`) and fuses that with whatever the
`RouteRegistry` and `ProviderRegistry` already hold.

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

<Note>
Codex's `config.toml` may show unrelated diff hunks on the first run —
that's `toml::to_string_pretty` re-formatting the existing file. Commit
the new file once and you're set; subsequent runs produce no diff.
</Note>

## Run

```bash
# stdio (default) — the client spawns the binary and speaks JSON-RPC
# over its stdin/stdout.
nestrs-mcp

# Streamable HTTP — the binary listens on <addr> and serves the
# MCP endpoint at /mcp. Requires `--features http` at install time.
nestrs-mcp --transport http --http-addr 127.0.0.1:7777
```

The HTTP transport is a `StreamableHttpService` mounted on an `axum::Router`
at `/mcp`.

## Security

**The HTTP transport has no built-in auth.** `nestrs-mcp --transport http`
binds to the address you give it and serves `/mcp` to anyone who can reach
it. Every tool — including the destructive scaffolding actions
(`new_project`, `create_module`, `create_resource`, `create_dto`,
`generate_crud`) — is reachable. This is intentional for v1 (the
recommended path is `stdio` with a local subprocess), but it is a real
gap before exposing `:7777` to anything beyond localhost.

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

`nestrs-mcp` speaks the standard Model Context Protocol, so any
MCP-aware client can use it. The two patterns are **stdio** (the client
spawns the binary as a subprocess) and **Streamable HTTP** (the client
connects to a running server).

### stdio (local)

The client launches `nestrs-mcp` on demand and pipes JSON-RPC through its
stdin/stdout. This is the recommended path for local dev — no ports, no
auth, no leftover processes.

**Claude Code** — add to `.mcp.json` in your project root (or
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

**VS Code (GitHub Copilot Chat)** — `.vscode/mcp.json` in your workspace:

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

After saving, restart the client (or click "Refresh" in the MCP servers
panel). The nestrs tools (`list_modules`, `get_app_health`,
`create_resource`, `search_docs`, …) appear in the model's tool list and
the model calls them automatically.

### Streamable HTTP (networked / hosted)

Useful when the binary runs on a host the client can't shell into, or
when several clients should share one server.

Start the server (it stays in the foreground; run it under your process
supervisor of choice):

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

From the shell, a quick sanity check that the HTTP transport is alive:

```bash
curl -sS -X POST http://127.0.0.1:7777/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0.0.1"}}}'
```

A `200 OK` with an `mcp-session-id` header and a JSON `result` block
means the handshake succeeded and the client can call tools.

## Features

- `stdio` (default) — stdio transport, no extra dependencies.
- `http` — Streamable HTTP transport. Pulls in
  `rmcp/transport-streamable-http-server` and `axum`.
- `admin` — enables the `nestrs-mcp::runtime::AdminClient` used by the
  live-runtime tools (`get_app_health`, `get_app_routes`,
  `get_app_providers`) to talk to a running nestrs app's
  `__nestrs/{health,routes,providers}` sidecar (the `admin` feature on the
  app side).

## Talking to a running nestrs app

The live-runtime tools hit a localhost-only sidecar exposed by
`NestApplication::use_admin(AdminOptions)` in the `nestrs` crate's `admin`
feature. To enable it, the app's `Cargo.toml` needs:

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

Then point the model's runtime tools at the same address + token.

## License

MIT OR Apache-2.0
