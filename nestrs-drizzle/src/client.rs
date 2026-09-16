//! Drizzle connection configuration and `DrizzleService` injectable.
//!
//! Mirrors the Mongoose-style pattern from `nestrs-mongodb` — a static
//! `for_root` setter establishes the connection URL once at boot, and a
//! `DrizzleService` is what callers resolve out of the DI container.
//! Drizzle itself is a query-builder DSL over the upstream sqlx pools,
//! so `DrizzleService` is intentionally thin: callers use it to access
//! the configured URL (which they then hand to drizzle-orm's typed
//! query helpers).

use crate::error::{DrizzleError, Result};
use std::sync::OnceLock;
use std::time::Duration;

/// Configuration for a Drizzle connection.
///
/// Holds the parsed connection URL plus optional driver-tuning knobs.
/// Drizzle itself is a query-builder, so most of the heavy lifting
/// happens at the underlying `sqlx::Pool` level — `DrizzleOptions`
/// captures the knobs the average app actually reaches for.
///
/// Drivers are selected via feature flags on the crate:
/// `postgres` / `mysql` / `sqlite` / `all`.
#[derive(Debug, Clone)]
pub struct DrizzleOptions {
    /// Full connection URL
    /// (`postgres://…`, `mysql://…`, `sqlite://…`, etc.).
    pub url: String,
    /// Pool size cap. Forwarded to the underlying `sqlx::Pool`.
    pub max_pool_size: Option<u32>,
    /// Connect / acquire timeout for a pooled connection.
    pub connect_timeout: Option<Duration>,
    /// `true` if this connection talks to a SQLite file (skips URL-based
    /// backend detection).
    pub sqlite: bool,
    /// `true` if this connection talks to a Postgres server.
    pub postgres: bool,
    /// `true` if this connection talks to a MySQL server.
    pub mysql: bool,
}

impl DrizzleOptions {
    /// Start a builder from a connection URL. The driver is auto-detected
    /// from the URL scheme (`postgres://`, `mysql://`, `sqlite://`,
    /// `sqlite:`).
    pub fn new(url: impl Into<String>) -> Self {
        let url = url.into();
        let postgres = url.starts_with("postgres://") || url.starts_with("postgresql://");
        let mysql = url.starts_with("mysql://") || url.starts_with("mariadb://");
        let sqlite = url.starts_with("sqlite://") || url.starts_with("sqlite:");
        Self {
            url,
            max_pool_size: None,
            connect_timeout: None,
            sqlite,
            postgres,
            mysql,
        }
    }

    /// Cap the underlying sqlx pool size.
    pub fn max_pool_size(mut self, n: u32) -> Self {
        self.max_pool_size = Some(n);
        self
    }

    /// Set the connect / acquire timeout for a pooled connection.
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = Some(d);
        self
    }

    /// Parse the URL into the components drizzle-orm / sqlx expect.
    /// Returns an error if the URL is malformed.
    pub fn parsed(&self) -> Result<url::Url> {
        url::Url::parse(&self.url).map_err(|e| DrizzleError::InvalidUrl(e.to_string()))
    }
}

/// Global connection URL set by `DrizzleModule::for_root`.
static DRIZZLE_OPTIONS: OnceLock<DrizzleOptions> = OnceLock::new();

/// Injectable Drizzle service. Holds no state of its own — callers use
/// it to ask for the configured URL (which they then hand to
/// `drizzle_orm::*::new(pool)`) and the resolved driver feature flag
/// (`postgres`, `mysql`, `sqlite`).
#[derive(Clone, Default)]
pub struct DrizzleService;

impl DrizzleService {
    /// Construct a service handle.
    pub fn new() -> Self {
        Self
    }

    /// Borrow the configured [`DrizzleOptions`] snapshot.
    pub fn options(&self) -> Result<&'static DrizzleOptions> {
        DRIZZLE_OPTIONS.get().ok_or(DrizzleError::NotConfigured)
    }

    /// Borrow the raw connection URL string.
    pub fn url(&self) -> Result<String> {
        Ok(self.options()?.url.clone())
    }

    /// `true` if the configured URL points to a Postgres server.
    pub fn is_postgres(&self) -> bool {
        self.options().map(|o| o.postgres).unwrap_or(false)
    }

    /// `true` if the configured URL points to a MySQL server.
    pub fn is_mysql(&self) -> bool {
        self.options().map(|o| o.mysql).unwrap_or(false)
    }

    /// `true` if the configured URL points to a SQLite database.
    pub fn is_sqlite(&self) -> bool {
        self.options().map(|o| o.sqlite).unwrap_or(false)
    }

    /// Resolve a `url::Url` for the configured connection. Surfaces the
    /// `InvalidUrl` variant if the configured URL is malformed (shouldn't
    /// happen unless someone set it manually outside `for_root`).
    pub fn parsed_url(&self) -> Result<url::Url> {
        self.options()?.parsed()
    }
}

impl std::fmt::Debug for DrizzleService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrizzleService").finish()
    }
}

/// Static module that owns the connection URL. `DrizzleModule::for_root`
/// is the pre-app config step; `DrizzleModule` itself is what you list
/// under `#[module(imports = …)]`.
pub struct DrizzleModule;

impl DrizzleModule {
    /// Static URL setter. Must be called exactly once, before
    /// `NestFactory::create`.
    ///
    /// ```ignore
    /// fn main() {
    ///     nestrs_drizzle::DrizzleModule::for_root("postgres://user:pass@localhost/app");
    ///     NestFactory::create::<AppModule>().listen(3000).await;
    /// }
    /// ```
    pub fn for_root(url: impl Into<String>) -> Self {
        let opts = DrizzleOptions::new(url);
        let _ = DRIZZLE_OPTIONS.set(opts);
        Self
    }

    /// Same as [`for_root`] but takes a fully-built [`DrizzleOptions`]
    /// (typically constructed from a `ConfigService` snapshot).
    pub fn for_root_with_options(opts: DrizzleOptions) -> Self {
        let _ = DRIZZLE_OPTIONS.set(opts);
        Self
    }
}

impl std::fmt::Debug for DrizzleModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrizzleModule").finish()
    }
}