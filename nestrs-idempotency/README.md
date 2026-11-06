# nestrs-idempotency

NestJS [`@nestjs/idempotency`](https://docs.nestjs.com/reliability/idempotency) analogue — `Idempotency-Key` request deduplication for NestRS.

```toml
nestrs-idempotency = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["idempotency"] }
```

## Features

- In-memory store (default) and optional Redis (`cache-redis`)
- Principal **scope** prefix so tenants cannot share keys
- Middleware replays completed responses; in-flight keys return **409**
- Exports `IdempotencyOptions` from `IdempotencyModule::register` for DI

## Docs

- Mintlify guide: [Idempotency](https://docs.nestrs.dev/ecosystem/idempotency) (path: `mintlify-docs/ecosystem/idempotency.mdx`)
- API: `mintlify-docs/api/crates/nestrs-idempotency.mdx`
- rustdoc: <https://docs.rs/nestrs-idempotency/1.7.0>
- Security: `docs/src/nestjs12-security.md`
