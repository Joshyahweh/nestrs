use nestrs_outbox::OutboxRelay;
use std::sync::Arc;

#[derive(Clone)]
pub struct WebhooksOptions {
    pub secret: String,
    pub outbox: Option<Arc<OutboxRelay>>,
}

impl WebhooksOptions {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            outbox: None,
        }
    }
}
