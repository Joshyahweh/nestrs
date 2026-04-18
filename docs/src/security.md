# Security

nestrs surfaces **security-sensitive behavior** through explicit APIs (CSRF, CORS, cookies, sessions, headers) and **runtime warnings** when a production-shaped environment variable set suggests a risky combination. The long-form policy, disclosure process, and control descriptions live in the repository’s **`SECURITY.md`**, included below.

## Quick checklist (before the full policy)

1. **Cookies + mutations**: If you use cookie sessions in browsers, pair them with a CSRF strategy—see [Secure defaults](secure-defaults.md) and the `csrf` feature.  
2. **CORS**: Do not ship permissive `*` origins in production; use environment-specific allowlists.  
3. **Headers**: Enable `use_security_headers` (or equivalent) for browser-facing APIs.  
4. **Errors**: Enable production error sanitization so stack traces do not leak to clients.  
5. **Dependencies**: Run `cargo audit` (or your org’s scanner) on a schedule; nestrs CI includes security workflows you can mirror locally.  

For **OpenAPI security schemes** and **`#[roles]`** documentation hints, see [OpenAPI & HTTP](openapi-http.md). For **`use_cookies`**, **`use_csrf_protection`**, **`use_security_headers`**, and related **`NestApplication`** snippets, see the [API cookbook](appendix-api-cookbook.md).

---

{{#include ../../SECURITY.md}}

