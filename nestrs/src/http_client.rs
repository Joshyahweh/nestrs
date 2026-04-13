//! NestJS **`HttpModule` / `HttpService`** analogue (feature: **`http-client`**).

use crate::module;
use nestrs_core::{Injectable, ProviderRegistry};
use std::sync::Arc;

/// Shared **`reqwest::Client`** for outbound HTTP (inject where needed).
pub struct HttpService {
    client: reqwest::Client,
}

impl Injectable for HttpService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|e| panic!("nestrs HttpService: reqwest::Client::build failed: {e}"));
        Arc::new(Self { client })
    }
}

impl HttpService {
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub fn get(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    pub fn post(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    pub fn put(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.put(url)
    }

    pub fn patch(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.patch(url)
    }

    pub fn delete(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.delete(url)
    }
}

/// Registers a singleton [`HttpService`] (and re-exports it).
#[module(providers = [HttpService], exports = [HttpService])]
pub struct HttpModule;

impl HttpModule {
    pub fn register() -> Self {
        Self
    }
}
