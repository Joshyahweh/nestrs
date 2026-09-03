//! HTTP client for the `nestrs::admin` sidecar port.
//!
//! `reqwest` over `rustls-tls`, plain `Authorization: Bearer <token>` if
//! a token is configured. No retries — admin-port calls are user-initiated
//! and the model can decide whether to re-invoke.

use std::time::Duration;

use crate::introspection::registry::SnapshotError;
use crate::Result;

use super::types::{AdminHealth, AdminProviders, AdminRoutes};

#[derive(Debug, Clone)]
pub struct AdminClient {
    base_url: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl AdminClient {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| crate::Error::Network(e.to_string()))?;
        Ok(Self {
            base_url: base_url.into(),
            token,
            http,
        })
    }

    pub async fn health(&self) -> Result<AdminHealth> {
        let url = format!("{}/__nestrs/health", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(self.token.as_deref().unwrap_or(""))
            .send()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SnapshotError::Http {
                status: status.as_u16(),
                body,
            }
            .into());
        }
        resp.json::<AdminHealth>()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))
    }

    pub async fn routes(&self) -> Result<AdminRoutes> {
        let url = format!("{}/__nestrs/routes", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(self.token.as_deref().unwrap_or(""))
            .send()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SnapshotError::Http {
                status: status.as_u16(),
                body,
            }
            .into());
        }
        resp.json::<AdminRoutes>()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))
    }

    pub async fn providers(&self) -> Result<AdminProviders> {
        let url = format!("{}/__nestrs/providers", self.base_url);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(self.token.as_deref().unwrap_or(""))
            .send()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SnapshotError::Http {
                status: status.as_u16(),
                body,
            }
            .into());
        }
        resp.json::<AdminProviders>()
            .await
            .map_err(|e| crate::Error::Network(e.to_string()))
    }
}
