# nestrs-workflows

NestJS `@nestjs/workflows` analogue — durable steps, resume after restart, human approval.

```toml
nestrs-workflows = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["workflows"] }
```

`start` → `complete_step` → `wait_for_approval` / `approve`. Store survives logical process restart.

## Docs

- Mintlify: `mintlify-docs/ecosystem/workflows.mdx`
- API: `mintlify-docs/api/crates/nestrs-workflows.mdx`
- rustdoc: <https://docs.rs/nestrs-workflows/1.7.0>
