//! Outbound HTTP client for the [`nestrs`](https://crates.io/crates/nestrs)
//! framework — the Rust equivalent of NestJS's
//! [`@nestjs/axios`](https://docs.nestjs.com/techniques/http-module).
//!
//! See the [`README`](https://github.com/Joshyahweh/nestrs/tree/main/nestrs-http)
//! for the high-level API. The umbrella's pre-extraction `nestrs::HttpService`
//! (in `nestrs/src/http_client.rs`) becomes a 1-line shim over this crate;
//! the real implementation lives here.
//!
//! ## Feature flags
//! - `default = []` — base crate.
//! - `reqwest` — re-exports `reqwest` at the crate root.

#![deny(missing_docs)]

use nestrs_core::{Injectable, ProviderRegistry};
use std::sync::Arc;

/// Default overall timeout for every request issued through [`HttpService`].
///
/// reqwest has **no** default timeout — without this, a TCP-connected but
/// unresponsive upstream hangs the handler future forever, and concurrent
/// hung calls accumulate until the runtime is saturated.
pub const DEFAULT_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Default TCP/TLS connect timeout for [`HttpService`] requests.
pub const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Shared [`reqwest::Client`] for outbound HTTP (inject where needed).
pub struct HttpService {
    client: reqwest::Client,
}

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
/// # use nestrs_http::{HttpService, HttpServiceOptions};
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
            .unwrap_or_else(|e| panic!("nestrs_http HttpService: reqwest::Client::build failed: {e}"));
        Self { client }
    }

    /// Borrow the underlying `reqwest::Client`. Most callers won't need this
    /// — the typed wrapper exposes every operation we ship — but advanced
    /// users (custom middleware, retry policies, interceptors) can drop down.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Start a `GET` request to `url`.
    pub fn get(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    /// Start a `POST` request to `url`.
    pub fn post(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.post(url)
    }

    /// Start a `PUT` request to `url`.
    pub fn put(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.put(url)
    }

    /// Start a `PATCH` request to `url`.
    pub fn patch(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.patch(url)
    }

    /// Start a `DELETE` request to `url`.
    pub fn delete(&self, url: impl reqwest::IntoUrl) -> reqwest::RequestBuilder {
        self.client.delete(url)
    }
}

/// Registers a singleton [`HttpService`] (and re-exports it). Mirror of the
/// pre-extraction `nestrs::HttpModule` shape.
pub struct HttpModule;

impl HttpModule {
    /// Install the module — registers the singleton [`HttpService`]
    /// provider.
    pub fn register() -> Self {
        Self
    }
}

impl std::fmt::Debug for HttpModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpModule").finish()
    }
}

impl std::fmt::Debug for HttpService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpService").finish_non_exhaustive()
    }
}

#[cfg(feature = "reqwest")]
pub use reqwest;