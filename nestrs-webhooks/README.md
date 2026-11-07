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
    let signed = svc.sign("msg_01", 1_700_000_000, body);
    svc.verify(
        "msg_01",
        "1700000000",
        &signed.signature_header,
        body,
    )
    .expect("valid");
}

// Or free functions:
let signed = sign_standard_webhook("whsec_…", "msg_01", 1_700_000_000, body);
verify_standard_webhook(
    "whsec_…",
    "msg_01",
    "1700000000",
    &signed.signature_header,
    body,
)
.expect("valid");
```

**Always verify against the raw request body** (before JSON parsing). Property tests live in `tests/sign_verify_prop.rs`.

## Surface

- `WebhooksModule` / `WebhooksOptions` / `WebhooksService`
- `sign_standard_webhook` / `SignedWebhook`
- `verify_standard_webhook` / `WebhookVerifyError`

## Docs

- Guide: [Webhooks](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/webhooks.mdx)
- Security: [reliability-security](https://github.com/Joshyahweh/nestrs/blob/main/docs/src/reliability-security.md)
- rustdoc: <https://docs.rs/nestrs-webhooks/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
