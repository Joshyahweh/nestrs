# Secure defaults checklist

nestrs is **explicit over magical**: many protections are **opt-in** so small services stay simple. Use this checklist when hardening browser-facing or multi-tenant HTTP APIs.

**Examples:** [`enable_cors`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.enable_cors), [`use_security_headers`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_security_headers), [`use_body_limit`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_body_limit), [`use_csrf_protection`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_csrf_protection), and related builders are shown in the [API cookbook](appendix-api-cookbook.md) (same names as in the matrix below).

## Production-shaped starter (illustrative)

Adapt names and policies to your threat model—this is a **memory aid**, not a drop-in `main.rs`:

1. Load **`TracingConfig`** and **`OpenTelemetryConfig`** (if using OTLP) from environment.  
2. Call **`enable_cors`** with an explicit origin list for browser clients; omit CORS for server-only APIs.  
3. **`use_security_headers`** (or `helmet_like()`) for HTML or browser-heavy APIs.  
4. **`use_body_limit`** and **`use_request_timeout`** on public endpoints.  
5. **`use_rate_limit`** (or enforce limits at the edge) for anonymous traffic.  
6. If you use **cookies** or **sessions**, enable **`csrf`** and **`use_csrf_protection`** for unsafe methods.  
7. **`enable_production_errors_from_env`** (or equivalent) so clients never see raw stack traces.  

Cross-check each row against the tables below when something behaves differently in `production` vs `development`.

## Runtime warnings you should understand

| Condition | What happens | What to do |
|-----------|----------------|------------|
| `use_cookies()` or `use_session_memory()` without `use_csrf_protection` (and feature `csrf` enabled) | `tracing` **WARN** at router build | Enable `csrf` + `use_cookies()` + `use_csrf_protection(...)` for cookie session flows that accept browser POSTs |
| Cookies/sessions with **`csrf` feature disabled** on the `nestrs` dependency | `tracing` **WARN** at router build | Add `features = ["csrf"]` (implies `cookies`) if you need built-in double-submit CSRF |
| `enable_cors` with **permissive** policy in a **production** environment (`NESTRS_ENV` / `APP_ENV` / `RUST_ENV`) | `tracing` **WARN** | Replace with explicit `CorsOptions` allowlists |

## CORS quick reference

- **Default**: no CORS layer until you call `NestApplication::enable_cors(...)`.
- **Browser + credentials** (`Access-Control-Allow-Credentials: true`): you **cannot** use `*` for origins — browsers reject it. Set explicit `allow_origins`.
- **API-only clients** (server-to-server, mobile native, CLI): CORS may be irrelevant; do not enable permissive CORS “just in case”.
- Prefer **environment-specific** `CorsOptions` loaded from config, not literals committed for production.

## CSRF stance (summary)

- **Bearer tokens in `Authorization`** are typically **not** CSRF-bound the same way cookies are.
- **Cookie sessions in browsers** need a **CSRF token** pattern (or SameSite-only flows you have modeled explicitly).
- nestrs ships **double-submit CSRF** behind the **`csrf`** feature; see `SECURITY.md` in the repo root.

## Secure-by-default matrix

| Control | Default in nestrs | Recommended for public web APIs |
|---------|-------------------|----------------------------------|
| HTTPS / TLS | Outside the library (reverse proxy) | Terminate TLS at the edge or process |
| CORS | Off until configured | Explicit allowlist per environment |
| Security headers | Off until `use_security_headers` | Enable `SecurityHeaders` / `helmet_like()` + CSP |
| CSRF | Off (feature + API opt-in) | On for cookie auth + unsafe methods |
| Cookies / sessions | Off until `use_cookies` / `use_session_memory` | Pair with CSRF when browsers mutate state |
| Body size | Off until `use_body_limit` | Set per route class / deployment |
| Request timeout | Off until `use_request_timeout` | Set for public endpoints |
| Rate limit | Off until `use_rate_limit` | Set at edge + optionally in-app |
| Production error sanitization | Off until `enable_production_errors` / `..._from_env` | Enable in prod |
| JSON unknown keys on `#[dto]` | **Denied by default** | Use `#[dto(allow_unknown_fields)]` only when needed |

## Further reading

- [Security](security.md) (includes `SECURITY.md`)
- [OpenAPI & HTTP](openapi-http.md) for documented routes and optional security schemes
- `PRODUCTION_RUNBOOK.md` (repository root)
