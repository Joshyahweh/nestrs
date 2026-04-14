# nestrs-macros

**Procedural macros** for [nestrs](https://crates.io/crates/nestrs): `#[module]`, `#[controller]`, HTTP verbs (`#[get]`, `#[post]`, …), `#[injectable]`, validation/DTO helpers, WebSocket routing (`#[ws_routes]` + `#[use_ws_*]`), microservice patterns (`#[micro_routes]`, `#[message_pattern]`, `#[use_micro_*]`), and more.

This crate is a **proc-macro** dependency of `nestrs`; you do not usually add it explicitly unless you are experimenting with macro expansion or building a fork.

**Semver:** Macro **input** syntax (attributes you write) is treated as public API; see workspace **`STABILITY.md`**.

**Docs:** [docs.rs/nestrs-macros](https://docs.rs/nestrs-macros) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install (normal apps)

Prefer the umbrella crate:

```toml
nestrs = "0.3.2"
```

`nestrs` already depends on `nestrs-macros`.

## Install (macro-only experiments)

```toml
[dependencies]
nestrs-macros = "0.3.2"
```

## What you get (surface sketch)

Application code typically uses attributes from **`nestrs`**, which re-exports the macros:

```rust
use nestrs::prelude::*;

#[module(controllers = [ApiController], providers = [AppService])]
struct AppModule;

#[controller(prefix = "/api")]
#[routes(state = AppState)]
impl ApiController {
    #[get("/health")]
    async fn health() -> &'static str {
        "ok"
    }
}
```

See [docs.rs/nestrs](https://docs.rs/nestrs) for the full attribute reference.

## License

MIT OR Apache-2.0.
