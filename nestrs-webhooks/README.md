# nestrs-webhooks

Signed webhooks for [nestrs](https://crates.io/crates/nestrs) — NestJS `@nestjs/webhooks` / [Standard Webhooks](https://www.standardwebhooks.com/) analogue.

Sign outbound payloads, verify inbound requests on the **raw body**, and optionally enqueue delivery through [`nestrs-outbox`](https://crates.io/crates/nestrs-outbox).

## Install

```toml
[dependencies]
nestrs-webhooks = "1.7.1"
# pulls nestrs-outbox for optional durable dispatch
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["webhooks"] }
# enables `outbox` as well
```

## Register

```rust
use nestrs_webhooks::{WebhooksModule, WebhooksOptions};

#[module(imports = [WebhooksModule::register(WebhooksOptions::new("whsec_…"))])]
struct AppModule;
```

Pass `WebhooksOptions { secret, outbox: Some(relay) }` to enqueue signed deliveries on the outbox.

## Sign & verify

```rust
use nestrs_webhooks::{sign_standard_webhook, verify_standard_webhook, WebhooksService};
use std::sync::Arc;

fn roundtrip(svc: &WebhooksService, body: &[u8]) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    let signed = svc.sign("msg_01", now, body);
    svc.verify("msg_01", &now.to_string(), &signed.signature_header, body)
        .expect("valid");
}

// Or free functions. `verify_standard_webhook` rejects timestamps more than
// five minutes from now (Standard Webhooks replay window).
let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .expect("clock")
    .as_secs() as i64;
let signed = sign_standard_webhook("whsec_…", "msg_01", now, body);
verify_standard_webhook(
    "whsec_…",
    "msg_01",
    &now.to_string(),
    &signed.signature_header,
    body,
)
.expect("valid");
```

**Always verify against the raw request body** (before JSON parsing). A valid signature with a stale timestamp is still rejected. Property tests live in `tests/sign_verify_prop.rs`.

## Surface

- `WebhooksModule` / `WebhooksOptions` / `WebhooksService`
- `sign_standard_webhook` / `SignedWebhook`
- `verify_standard_webhook` / `verify_standard_webhook_at` / `WebhookVerifyError`
- `DEFAULT_WEBHOOK_TOLERANCE_SECS` (300)

## Docs

- Guide: [Webhooks](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/webhooks.mdx)
- Security: [reliability-security](https://github.com/Joshyahweh/nestrs/blob/main/docs/src/reliability-security.md)
- rustdoc: <https://docs.rs/nestrs-webhooks/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
