use crate::transport::{InMemoryMailTransport, SharedMailTransport};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct MailOptions {
    pub transport: SharedMailTransport,
}

impl Default for MailOptions {
    fn default() -> Self {
        Self {
            transport: InMemoryMailTransport::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailMessage {
    pub to: String,
    pub subject: String,
    pub html: String,
    pub idempotency_key: Option<String>,
}
