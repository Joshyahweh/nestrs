# nestrs-mail

Templated email for [nestrs](https://crates.io/crates/nestrs) — NestJS [`@nestjs/mail`](https://docs.nestjs.com/techniques/mail) analogue.

HTML-escaped `{{placeholders}}`, pluggable transports, and an **in-memory inbox** for tests. Optional HTTP transport for Resend / SendGrid–style APIs.

## Install

```toml
[dependencies]
nestrs-mail = "1.7.1"
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["mail"] }
# HTTP transport:
# nestrs-mail = { version = "1.7.1", features = ["http"] }
```

## Register & send

```rust
use nestrs_mail::{
    render_template, InMemoryMailTransport, MailMessage, MailModule, MailOptions, MailService,
};
use std::collections::HashMap;
use std::sync::Arc;

fn mail_module() -> nestrs_core::DynamicModule {
    let inbox = InMemoryMailTransport::new();
    MailModule::register(MailOptions {
        transport: inbox,
    })
}

async fn welcome(mail: Arc<MailService>) {
    let mut data = HashMap::new();
    data.insert("name".into(), "<b>Ada</b>".into());
    let html = render_template("<p>Hi {{name}}</p>", &data);
    // → "<p>Hi &lt;b&gt;Ada&lt;/b&gt;</p>"

    mail.send(MailMessage {
        to: "ada@example.com".into(),
        subject: "Welcome".into(),
        html,
        idempotency_key: Some("welcome:ada".into()),
    })
    .await
    .unwrap();
}
```

### Transports

| Transport | Feature | Role |
|-----------|---------|------|
| `InMemoryMailTransport` | default | Test inbox (`inbox()` / `clear()`) |
| `HttpMailTransport` | `http` | Bearer-auth POST to a provider endpoint |
| Custom | — | Implement `MailTransport` |

## Surface

- `MailModule` / `MailOptions` / `MailService`
- `MailMessage`, `MailError`
- `render_template`
- `InMemoryMailTransport`, `HttpMailTransport`, `MailTransport`

## Docs

- Guide: [Mail](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/mail.mdx)
- rustdoc: <https://docs.rs/nestrs-mail/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
