use crate::{MailError, MailMessage};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

#[async_trait]
pub trait MailTransport: Send + Sync {
    async fn send(&self, message: MailMessage) -> Result<(), MailError>;
}

pub type SharedMailTransport = Arc<dyn MailTransport>;

pub struct InMemoryMailTransport {
    messages: Mutex<Vec<MailMessage>>,
}

impl InMemoryMailTransport {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            messages: Mutex::new(Vec::new()),
        })
    }

    pub async fn inbox(&self) -> Vec<MailMessage> {
        self.messages.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.messages.lock().await.clear();
    }
}

#[async_trait]
impl MailTransport for InMemoryMailTransport {
    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        self.messages.lock().await.push(message);
        Ok(())
    }
}

/// Minimal HTTP mailer (Resend/SendGrid-style POST). Enable with feature `http`.
#[cfg(feature = "http")]
pub struct HttpMailTransport {
    pub endpoint: String,
    pub api_key: String,
    pub client: reqwest::Client,
}

#[cfg(feature = "http")]
impl HttpMailTransport {
    pub fn new(endpoint: impl Into<String>, api_key: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        })
    }
}

#[cfg(feature = "http")]
#[async_trait]
impl MailTransport for HttpMailTransport {
    async fn send(&self, message: MailMessage) -> Result<(), MailError> {
        let mut req = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&serde_json::json!({
                "to": message.to,
                "subject": message.subject,
                "html": message.html,
            }));
        if let Some(key) = &message.idempotency_key {
            req = req.header("Idempotency-Key", key);
        }
        let res = req
            .send()
            .await
            .map_err(|e| MailError::Transport(e.to_string()))?;
        if res.status().is_client_error() && res.status().as_u16() != 429 {
            return Err(MailError::Permanent(format!("mail http {}", res.status())));
        }
        if !res.status().is_success() {
            return Err(MailError::Transport(format!("mail http {}", res.status())));
        }
        Ok(())
    }
}
