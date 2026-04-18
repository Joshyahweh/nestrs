# Security Guide

This document captures the current nestrs security baseline and production guidance.

## Reporting vulnerabilities

Please report security vulnerabilities privately.

- Preferred: GitHub Security Advisories for this repository
  - <https://github.com/Joshyahweh/nestrs/security/advisories/new>
- Do **not** open public issues for active vulnerabilities.

### What to include in a report

- Affected component/crate and version
- Reproduction details or proof-of-concept
- Impact assessment (confidentiality/integrity/availability)
- Suggested mitigation if known

### Response expectations

- Initial triage acknowledgement target: within 72 hours
- Coordinated fix and disclosure timeline based on severity
- Credit in release notes/changelog when appropriate

## Current platform controls

- **CORS**: configurable through `NestApplication::enable_cors(CorsOptions::...)`. Prefer **explicit origins** in production; permissive / wildcard settings are easy to misconfigure for browser clients that send credentials. See the mdBook page **`docs/src/secure-defaults.md`** (section *CORS quick reference*) for a short checklist.
- **Security headers**: configurable through `NestApplication::use_security_headers(SecurityHeaders::default()...)`; opt-in **`SecurityHeaders::helmet_like()`** adds Cross-Origin-Opener-Policy, Cross-Origin-Resource-Policy, `X-DNS-Prefetch-Control`, and related Helmet-style headers (still configure CSP/HSTS yourself).
- **Rate limiting hooks**: configurable through `NestApplication::use_rate_limit(RateLimitOptions::...)`.
- **Body limits**: configurable through `NestApplication::use_body_limit(...)`.
- **Timeouts**: configurable through `NestApplication::use_request_timeout(...)`.
- **Secure 5xx responses**: `NestApplication::enable_production_errors()` sanitizes internal details.
- **Environment-based secure errors**: `enable_production_errors_from_env()` uses `NESTRS_ENV`/`APP_ENV`/`RUST_ENV`.
- **Authentication building blocks**: `CanActivate` guards, `AuthStrategy`, `#[roles(...)]` metadata, `XRoleMetadataGuard`, `AuthStrategyGuard`, and Axum extractors `BearerToken` / `OptionalBearerToken` (no bundled Passport/JWT — use your crates of choice).
- **CSRF (opt-in)**: feature **`csrf`** + `NestApplication::use_csrf_protection` with **`use_cookies()`** — double-submit check on POST/PUT/PATCH/DELETE (`CsrfProtectionConfig`).

## Encryption and hashing

- nestrs does **not** ship password hashing, AEAD, or key management. Use well-vetted crates (for example `argon2`, `bcrypt`, `ring`, or `aes-gcm`) and your platform’s secret stores.

## Production recommendations

- Prefer explicit CORS allowlists over permissive mode.
- Keep `SecurityHeaders::default()` enabled and set a CSP for browser-facing apps.
- Use strict request body limits and timeout values per deployment profile.
- Turn on `enable_production_errors_from_env()` in production workloads.
- Use `use_request_id()` and request tracing for incident correlation.

## TLS stance

- nestrs is designed to run behind a TLS-terminating reverse proxy/load balancer in production.
- If TLS is terminated upstream, keep HSTS/header policy configured at the edge and/or app layer.
- For direct internet exposure, terminate TLS before or at the nestrs process and rotate certificates regularly.

## CSRF stance

- **Bearer token in `Authorization` header**: CSRF is generally not applicable.
- **Cookie-based session auth**: CSRF protection is required (token-based anti-CSRF pattern recommended).
- nestrs does **not** enable CSRF by default. When you need it, enable feature **`csrf`**, call **`use_cookies()`**, then **`use_csrf_protection(CsrfProtectionConfig::...)`** — your app must issue the token (e.g. set the cookie on a safe request and require the same value in `X-CSRF-Token` on mutations).

### Runtime diagnostics

When cookies or in-memory sessions are enabled **without** CSRF wiring, `NestApplication::build_router` emits a **`tracing` WARN** (and a separate WARN if the `csrf` Cargo feature is missing entirely). Treat these as release-blocking for browser-facing cookie auth until you have an explicit CSRF or SameSite strategy.

## Secrets and config

- Do not commit secrets to source control.
- Load secrets from environment variables and secret managers.
- Use `.env.example` only for non-sensitive placeholders.
- Ensure logs do not include secret values (tokens, passwords, API keys).

## Supply chain checks

- CI includes a `cargo audit` workflow at `.github/workflows/security.yml`.
- Run `cargo audit` locally before releases.
- Keep `Cargo.lock` committed and dependencies updated.
