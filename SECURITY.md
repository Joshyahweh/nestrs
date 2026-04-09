# Security Guide

This document captures the current nestrs security baseline and production guidance.

## Current platform controls

- **CORS**: configurable through `NestApplication::enable_cors(CorsOptions::...)`.
- **Security headers**: configurable through `NestApplication::use_security_headers(SecurityHeaders::default()...)`.
- **Rate limiting hooks**: configurable through `NestApplication::use_rate_limit(RateLimitOptions::...)`.
- **Body limits**: configurable through `NestApplication::use_body_limit(...)`.
- **Timeouts**: configurable through `NestApplication::use_request_timeout(...)`.
- **Secure 5xx responses**: `NestApplication::enable_production_errors()` sanitizes internal details.
- **Environment-based secure errors**: `enable_production_errors_from_env()` uses `NESTRS_ENV`/`APP_ENV`/`RUST_ENV`.

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
- nestrs does not currently auto-enable a CSRF middleware by default; apply one when using cookie sessions.

## Secrets and config

- Do not commit secrets to source control.
- Load secrets from environment variables and secret managers.
- Use `.env.example` only for non-sensitive placeholders.
- Ensure logs do not include secret values (tokens, passwords, API keys).

## Supply chain checks

- CI includes a `cargo audit` workflow at `.github/workflows/security.yml`.
- Run `cargo audit` locally before releases.
- Keep `Cargo.lock` committed and dependencies updated.
