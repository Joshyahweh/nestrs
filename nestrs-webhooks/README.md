# nestrs-webhooks

NestJS `@nestjs/webhooks` analogue — Standard Webhooks signing, inbound verify, outbound via outbox.

```toml
nestrs-webhooks = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["webhooks"] }
```

Verify on the **raw body**. Optional `WebhooksOptions.outbox` for durable dispatch.

## Docs

- Mintlify: `mintlify-docs/ecosystem/webhooks.mdx`
- API: `mintlify-docs/api/crates/nestrs-webhooks.mdx`
- Property tests: `tests/sign_verify_prop.rs`
- rustdoc: <https://docs.rs/nestrs-webhooks/1.7.0>
