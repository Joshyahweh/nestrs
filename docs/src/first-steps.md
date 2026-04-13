# First steps

This guide is the **nestrs** counterpart to Nest’s *Introduction* and *First steps*: enough to run a small HTTP API and know where to read next. It is **Partial** parity with [docs.nestjs.com](https://docs.nestjs.com) — you get working Rust patterns and links into this book and the repo, not Nest’s full onboarding (videos, StackBlitz-style sandboxes, or a chapter-for-chapter clone of their narrative).

## What you need

- **Rust** toolchain (`rustup`, stable recommended).
- A **nestrs** dependency (workspace crate or crates.io), typically with `tokio` for `async` main.

## 1) Create or open a project

**Option A — CLI scaffold** (closest to `nest new`):

```bash
cargo install nestrs-scaffold   # binary name: nestrs
nestrs new my-api
cd my-api
```

See [CLI](cli.md) and `nestrs-cli/README.md` for `nestrs generate` and templates.

**Option B — Cargo only**:

```bash
cargo new my-api
cd my-api
```

Add `nestrs` (and `tokio` with `macros` + `rt-multi-thread`) to `Cargo.toml`, then copy the minimal module below.

## 2) Minimal HTTP application

The smallest useful app: one **module**, one **controller**, one **route**, and **`NestFactory`**.

```rust
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct HelloController;

#[routes(state = AppState)]
impl HelloController {
    #[get("/hello")]
    async fn hello() -> &'static str {
        "Hello from nestrs"
    }
}

#[module(controllers = [HelloController], providers = [AppState])]
struct AppModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<AppModule>()
        .listen_graceful(3000)
        .await;
}
```

Run:

```bash
cargo run
```

Then open `http://127.0.0.1:3000/api/hello` (global prefix is optional; without `set_global_prefix`, the path is as composed from `#[controller(prefix = ...)]` and the route).

### Mental model (Nest → nestrs)

| Nest idea | nestrs |
|-----------|--------|
| `@Module` | `#[module(...)]` on a struct |
| `@Controller` / `@Get` | `#[controller(...)]` + `#[routes]` impl with `#[get("/path")]` |
| `@Injectable` provider | `#[injectable]` on a type; register in `providers = [...]` |
| `NestFactory.create` | `NestFactory::create::<AppModule>()` |

## 3) Richer example in the repo

The **`hello-app`** example adds **Prisma/SQLx**, **DTO validation**, **versioned routes**, and **redirect/header** helpers:

```bash
cargo run -p hello-app
```

Source: `examples/hello-app/src/main.rs`. Use it when you outgrow the snippet above.

## 4) Read next (by topic)

| Goal | Where |
|------|--------|
| CLI generators | [CLI](cli.md) |
| Guards, roles, headers, CSRF | [Security](security.md) |
| Metrics, tracing, production | [Observability](observability.md), [Production runbook](production.md) |
| Kafka / NATS / Redis / gRPC-style microservices | [Microservices](microservices.md) |
| Optional crates (GraphQL, OpenAPI, WS) | [Ecosystem modules](ecosystem.md) |
| Nest ↔ nestrs feature matrix | [Roadmap parity](roadmap-parity.md) |
| Metadata / “custom decorators” | [Custom decorators](custom-decorators.md) |
| Scopes, `ModuleRef`, dynamic modules, cycles | [Fundamentals](fundamentals.md) |

## 5) Build this book locally

From the repo root:

```bash
cd docs && mdbook serve
```

Open the URL mdbook prints (usually `http://127.0.0.1:3000`). The [Introduction](index.md) still embeds the repo `README.md` for project layout and CI pointers.

## Compared to Nest’s “full” first steps

**In nestrs today:**

- Documentation is **mdBook + README + per-crate rustdoc**, not a separate marketing/tutorial site with the same depth as Nest’s every sidebar page.
- **No** built-in interactive playground or hosted “try nestrs in the browser.”
- Onboarding assumes **comfort with Rust** (ownership, `async`, Cargo); we do not replicate Nest’s TypeScript-first teaching path.

**You still get:** a small working server in a few macros, a real example app, and explicit links for security, ops, and parity expectations.
