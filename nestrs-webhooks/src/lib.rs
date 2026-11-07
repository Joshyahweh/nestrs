//! NestJS [`@nestjs/webhooks`](https://docs.nestjs.com/techniques/webhooks) analogue —
//! [Standard Webhooks](https://www.standardwebhooks.com/) signing/verification and
//! optional outbox-backed outbound dispatch.
//!
//! Always verify on the **raw** request body. Prefer `whsec_`-encoded secrets and never
//! log them. Pair [`WebhooksOptions::outbox`] with [`nestrs_outbox::OutboxRelay`] for
//! durable delivery.
//!
//! Umbrella feature: `nestrs` / `webhooks` (implies `outbox`) → `nestrs::webhooks`.
#![doc(html_root_url = "https://docs.rs/nestrs-webhooks/1.7.1")]

mod module;
mod options;
mod sign;
mod verify;

pub use module::WebhooksModule;
pub use options::WebhooksOptions;
pub use sign::{sign_standard_webhook, SignedWebhook};
pub use verify::{verify_standard_webhook, WebhookVerifyError};

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
use nestrs_outbox::OutboxRelay;
use serde_json::json;
use std::sync::Arc;

pub struct WebhooksService {
    secret: String,
    outbox: Option<Arc<OutboxRelay>>,
}

impl WebhooksService {
    pub fn new(secret: impl Into<String>, outbox: Option<Arc<OutboxRelay>>) -> Self {
        Self {
            secret: secret.into(),
            outbox,
        }
    }

    /// Sign a payload for immediate HTTP delivery by the caller.
    pub fn sign(&self, msg_id: &str, timestamp: i64, body: &[u8]) -> SignedWebhook {
        sign_standard_webhook(&self.secret, msg_id, timestamp, body)
    }

    /// Enqueue an outbound webhook through the transactional outbox.
    pub async fn dispatch(
        &self,
        topic: &str,
        url: &str,
        body: serde_json::Value,
    ) -> Result<(), String> {
        let Some(outbox) = &self.outbox else {
            return Err("webhooks: outbox not configured".into());
        };
        outbox
            .add(
                topic,
                json!({
                    "url": url,
                    "body": body,
                }),
            )
            .await;
        Ok(())
    }

    pub fn verify(
        &self,
        msg_id: &str,
        timestamp: &str,
        signature: &str,
        body: &[u8],
    ) -> Result<(), WebhookVerifyError> {
        verify_standard_webhook(&self.secret, msg_id, timestamp, signature, body)
    }
}

impl Injectable for WebhooksService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("WebhooksService must be provided by WebhooksModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let secret = "whsec_test";
        let body = br#"{"hello":"world"}"#;
        let msg_id = "msg_1";
        let ts = 1_700_000_000_i64;
        let signed = sign_standard_webhook(secret, msg_id, ts, body);
        verify_standard_webhook(
            secret,
            msg_id,
            &ts.to_string(),
            &signed.signature_header,
            body,
        )
        .expect("verify");
    }
}
