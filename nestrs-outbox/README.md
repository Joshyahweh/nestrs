# nestrs-outbox

NestJS `@nestjs/outbox` analogue — transactional outbox, relay, retries, DLQ.

```toml
nestrs-outbox = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["outbox"] }
```

Enqueue with `OutboxRelay::add`, deliver with `tick(&PublishFn)`. Pair with `nestrs-webhooks` for signed outbound HTTP.

## Docs

- Mintlify: `mintlify-docs/ecosystem/outbox.mdx`
- API: `mintlify-docs/api/crates/nestrs-outbox.mdx`
- rustdoc: <https://docs.rs/nestrs-outbox/1.7.0>
