//! NestJS **`HttpModule` / `HttpService`** analogue (feature: **`http-client`**).

use crate::module;
use nestrs_core::{Injectable, ProviderRegistry};
use std::sync::Arc;

/// Shared **`reqwest::Client`** for outbound HTTP (inject where needed).
pub struct HttpService {
    client: reqwest::Client,
}

/// Default overall timeout for every request issued through [`HttpService`].
///
/// reqwest has **no** default timeout — without this, a TCP-connected but
/// unresponsive upstream hangs the handler future forever, and concurrent
/// hung calls accumulate until the runtime is saturated.
pub const DEFAULT_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Default TCP/TLS connect timeout for [`HttpService`] requests.
pub const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Tunables for the shared [`HttpService`] client.
///
/// All values are `Duration`s applied via reqwest's `timeout` (whole request)
/// and `connect_timeout` (TCP+TLS establishment). Callers can still override
/// the overall timeout per request with `.timeout()` on the returned builder.
///
/// `HttpModule` registers a service with these defaults. To inject a tuned
/// instance instead, build it with [`HttpService::from_options`] and register
/// the value (this wins over the module's default provider):
///
/// ```no_run
/// # use std::sync::Arc;
/// # use nestrs::{HttpService, HttpServiceOptions};
/// # use nestrs_core::ProviderRegistry;
/// let mut registry = ProviderRegistry::new();
/// let options = HttpServiceOptions {
///     request_timeout: std::time::Duration::from_secs(5),
///     ..Default::default()
/// };
/// registry.register_use_value::<HttpService>(Arc::new(HttpService::from_options(&options)));
/// ```
#[derive(Clone, Debug)]
pub struct HttpServiceOptions {
    /// Whole-request deadline (connect + writes + reads + body). Default
    /// [`DEFAULT_REQUEST_TIMEOUT`].
    pub request_timeout: std::time::Duration,
    /// TCP/TLS establishment deadline. Default [`DEFAULT_CONNECT_TIMEOUT`].
    pub connect_timeout: std::time::Duration,
}

impl Default for HttpServiceOptions {
    fn default() -> Self {
        Self {
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }
}

impl Injectable for HttpService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::from_options(&HttpServiceOptions::default()))
    }
}

impl HttpService {
    /// Build a service from explicit options.
    pub fn from_options(options: &HttpServiceOptions) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(options.connect_timeout)
            .timeout(options.request_timeout)
            .build()
            .unwrap_or_else(|e| panic!("nestrs HttpService: reqwest::Client::build failed: {e}"));
        Self { client }
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
