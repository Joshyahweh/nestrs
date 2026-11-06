# NestJS 12 parity — security notes

Threat model notes for the new reliability / AI packages (Wave 5).

## Idempotency (`nestrs-idempotency`)

- Always **scope** keys by authenticated principal (`IdempotencyOptions.scope`) so tenants cannot replay another user's response.
- Treat in-flight collisions as **409**, not 200 — never execute the handler twice for the same key.
- TTL must bound store growth; defaults to 24h in-memory.

## Webhooks (`nestrs-webhooks`)

- Secrets: prefer `whsec_` Standard Webhooks encoding; never log raw secrets (`Debug` should not print them in app code).
- Verify on the **raw body** before JSON parse.
- Outbound URLs from user input are SSRF-sensitive — validate allowlists at the application layer before `dispatch`.

## Outbox (`nestrs-outbox`)

- Poison messages: after `max_attempts`, messages move to DLQ — alert on `list_dead_letters`.
- In-memory store is for tests/dev; production should set `allow_in_memory: false` once a durable adapter is wired.

## AI tools (`nestrs-ai`)

- Agent tools must enforce the **caller's** authz (guards / `Ability`) — never run privileged tools anonymously.
- `FakeModel` is for tests only; never configure it in production binaries.

## Resilience (`nestrs-resilience`)

- Retry jitter is **on by default** to avoid thundering herds.
- Map `Timeout` → 504 and `CircuitOpen` → 503 + `Retry-After` at the HTTP boundary.

## Mail (`nestrs-mail`)

- Template render must escape HTML by default; never concatenate untrusted input into SMTP bodies without escaping.
- HTTP providers (SES/Resend-style) inherit webhook/HTTP SSRF rules — pin destinations; do not follow arbitrary redirect targets from user content.

## Locks / workflows (`nestrs-locks`, `nestrs-workflows`)

- Lease TTL must be shorter than the worst-case work you are willing to duplicate after a crash; extend leases on long steps.
- Approval waits are durable state — treat resume tokens as capability secrets (do not log them).
