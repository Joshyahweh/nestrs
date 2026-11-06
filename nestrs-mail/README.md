# nestrs-mail

NestJS `@nestjs/mail` analogue — escaped templates, SMTP/HTTP transports, in-memory inbox for tests.

```toml
nestrs-mail = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["mail"] }
```

`render_template("Hello {{name}}", &data)` HTML-escapes values. Use `InMemoryMailTransport` in tests.

## Docs

- Mintlify: `mintlify-docs/ecosystem/mail.mdx`
- API: `mintlify-docs/api/crates/nestrs-mail.mdx`
- rustdoc: <https://docs.rs/nestrs-mail/1.7.0>
