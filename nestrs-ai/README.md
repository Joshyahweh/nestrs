# nestrs-ai

NestJS [`@nestjs/ai`](https://docs.nestjs.com/ai/overview) analogue — named models, `generate_text` / `generate_object`, agent tools, `FakeModel`, resilience.

```toml
nestrs-ai = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["ai"] }
```

- `FakeModel` for tests/labs only
- `generate_object` fails closed on invalid JSON / missing required fields
- Tool handlers must enforce caller authz
- Model calls use `nestrs-resilience` timeout + jittered retry

## Docs

- Mintlify: `mintlify-docs/ecosystem/ai.mdx`
- API: `mintlify-docs/api/crates/nestrs-ai.mdx`
- Hub: `mintlify-docs/guides/reliability.mdx`
- rustdoc: <https://docs.rs/nestrs-ai/1.7.0>
