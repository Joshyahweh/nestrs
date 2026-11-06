//! NestJS [`@nestjs/mail`](https://docs.nestjs.com/techniques/mail) analogue —
//! templated email with HTML-escaped `{{placeholders}}`, pluggable transports,
//! and an in-memory inbox for tests.
//!
//! ```ignore
//! use nestrs_mail::{render_template, MailMessage, MailModule, MailOptions, MailService};
//! use std::collections::HashMap;
//!
//! let dm = MailModule::register(MailOptions::default());
//! # async fn send(mail: std::sync::Arc<MailService>) {
//! let mut data = HashMap::new();
//! data.insert("name".into(), "<b>Ada</b>".into());
//! let html = render_template("<p>Hi {{name}}</p>", &data);
//! mail.send(MailMessage {
//!     to: "a@b.com".into(),
//!     subject: "Hi".into(),
//!     html,
//!     idempotency_key: None,
//! }).await.unwrap();
//! # }
//! ```
//!
//! Umbrella feature: `nestrs` / `mail` → `nestrs::mail`.
#![doc(html_root_url = "https://docs.rs/nestrs-mail/1.7.0")]

mod module;
mod options;
mod transport;

pub use module::MailModule;
pub use options::{MailMessage, MailOptions};
#[cfg(feature = "http")]
pub use transport::HttpMailTransport;
pub use transport::{InMemoryMailTransport, MailTransport, SharedMailTransport};

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum MailError {
    Transport(String),
    Permanent(String),
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(m) | Self::Permanent(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for MailError {}

impl MailError {
    pub fn permanent(&self) -> bool {
        matches!(self, Self::Permanent(_))
    }
}

/// Escape and substitute `{{key}}` placeholders (Nest built-in engine analogue).
pub fn render_template(template: &str, data: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in data {
        let needle = format!("{{{{{k}}}}}");
        out = out.replace(&needle, &html_escape::encode_text(v));
    }
    out
}

pub struct MailService {
    transport: SharedMailTransport,
}

impl MailService {
    pub fn from_options(opts: &MailOptions) -> Self {
        Self {
            transport: opts.transport.clone(),
        }
    }

    pub async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        self.transport.send(message).await
    }

    /// Test helper when using [`InMemoryMailTransport`].
    pub fn transport(&self) -> SharedMailTransport {
        self.transport.clone()
    }
}

impl Injectable for MailService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("MailService must be provided by MailModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::InMemoryMailTransport;

    #[tokio::test]
    async fn renders_and_sends_to_memory() {
        let mem = InMemoryMailTransport::new();
        let svc = MailService {
            transport: mem.clone(),
        };
        let mut data = HashMap::new();
        data.insert("name".into(), "<script>".into());
        let body = render_template("Hello {{name}}", &data);
        assert!(body.contains("&lt;script&gt;"));
        svc.send(MailMessage {
            to: "a@b.com".into(),
            subject: "Hi".into(),
            html: body,
            idempotency_key: Some("k1".into()),
        })
        .await
        .unwrap();
        let inbox = mem.inbox().await;
        assert_eq!(inbox.len(), 1);
    }
}
