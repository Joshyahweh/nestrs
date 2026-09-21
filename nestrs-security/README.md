# nestrs-security

Security building blocks for [`nestrs`](https://docs.rs/nestrs) — Bearer token parsing,
extractor + guard helpers, the **helmet-style** response-header middleware, and the
double-submit CSRF middleware (NestJS [`helmet`](https://docs.nestjs.com/security/helmet)
+ [`csurf`](https://github.com/expressjs/csurf) parity).

Extracted from `nestrs/src/security/*` so apps can pull just this crate. The umbrella
`nestrs` crate re-exports every public symbol behind the same paths; existing apps do
not need to change their imports.

## What it provides

- **Auth helpers (default features):**
  - [`parse_authorization_bearer`](https://docs.rs/nestrs-security/latest/nestrs_security/fn.parse_authorization_bearer.html)
    — case-insensitive `Bearer` scheme parser.
  - [`BearerToken`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.BearerToken.html)
    / [`OptionalBearerToken`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.OptionalBearerToken.html)
    axum extractors.
  - [`AuthStrategyGuard<S>`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.AuthStrategyGuard.html)
    — runs [`AuthStrategy::validate`](https://docs.rs/nestrs-core/latest/nestrs_core/trait.AuthStrategy.html)
    for any `S: AuthStrategy + Default`.
  - [`DemoXRoleMetadataGuard`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.DemoXRoleMetadataGuard.html)
    — demo `#[roles("a,b")]` metadata check (client-trusted `x-role` header; **do
    not use in production**).
  - [`route_roles_csv`](https://docs.rs/nestrs-security/latest/nestrs_security/fn.route_roles_csv.html)
    / `route_metadata_csv` — read handler metadata from request parts.

- **CSRF middleware (feature `csrf`):**
  - [`CsrfProtectionConfig`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.CsrfProtectionConfig.html)
    + [`csrf_double_submit_middleware`](https://docs.rs/nestrs-security/latest/nestrs_security/fn.csrf_double_submit_middleware.html)
    — server-side cookie value vs `X-CSRF-Token` header value, constant-time
    compared. Skips safe methods (GET / HEAD / OPTIONS).

- **Helmet middleware (Phase D surface — default features):**
  - [`HelmetConfig`](https://docs.rs/nestrs-security/latest/nestrs_security/struct.HelmetConfig.html)
    + [`helmet_middleware`](https://docs.rs/nestrs-security/latest/nestrs_security/fn.helmet_middleware.html)
    — `X-Frame-Options` / `X-Content-Type-Options` / `Strict-Transport-Security` /
    `Referrer-Policy` / `X-DNS-Prefetch-Control` / `Cross-Origin-Opener-Policy`.

## Install

```toml
[dependencies]
nestrs-security = "1.0"

# CSRF middleware (pulls in tower-cookies + subtle):
nestrs-security = { version = "1.4.0", features = ["csrf"] }
```

## Reuse from the umbrella

Every symbol is re-exported at the same path on the `nestrs` crate behind the
**`security`** feature (default for `nestrs` is no security — opt in):

```toml
nestrs = { version = "1.4.0", features = ["security", "csrf"] }
```

`nestrs::parse_authorization_bearer`, `nestrs::BearerToken`, `nestrs::HelmetConfig`,
`nestrs::helmet_middleware`, `nestrs::csrf_double_submit_middleware`, etc. all
resolve through this crate.

## License

MIT OR Apache-2.0
