# nestrs-better-auth

Thin adapter that plugs [better-auth.rs](https://github.com/better-auth-rs/better-auth-rs)
into nestrs: nest its Axum router and guard nestrs handlers on the session cookie.

This crate does **not** vendor Better Auth plugins. Build `BetterAuth` upstream,
call `auth.axum_router()`, then:

```toml
nestrs-better-auth = "1.5.0"
```

```rust,ignore
use nestrs_better_auth::{BetterAuthGuard, BetterAuthModule, BetterAuthOptions};

#[module(imports = [BetterAuthModule::for_root(auth.axum_router(), BetterAuthOptions::default())])]
struct AppModule;

#[get("/me")]
#[use_guards(BetterAuthGuard)]
async fn me() -> &'static str { "ok" }
```
