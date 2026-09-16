//! MongoDB client lifecycle: connection options, module setup, and the
//! injectable `MongoService`.
//!
//! This module ships the Phase B surface: connection configuration, the
//! `MongoModule::for_root` static setter, and a `MongoService` that callers
//! can resolve out of the DI container. The typed `MongoRepository<T>`
//! CRUD wrapper lives in [`crate::repository`].
//!
//! ## Example
//!
//! ```ignore
//! use nestrs_mongodb::{MongoModule, MongoService};
//!
//! fn main() {
//!     MongoModule::for_root("mongodb://127.0.0.1:27017");
//!     let svc = MongoService::new();
//!     let db = svc.database("app");
//! }
//! ```

use crate::error::{MongoError, Result};
use mongodb::options::ClientOptions;
use mongodb::{Client, Database};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::OnceCell;

/// Configuration builder for a MongoDB connection.
///
/// Most apps only need [`MongoOptions::uri`]. The other fields exist so apps
/// can override the driver's defaults (app name for server logs, connect /
/// operation timeouts, direct-connection mode for test harnesses) without
/// dropping down to raw `mongodb::options::ClientOptions`.
///
/// Mirrors the relevant subset of `mongodb::options::ClientOptions` so
/// drivers can pass the constructed `ClientOptions` straight through.
#[derive(Debug, Clone)]
pub struct MongoOptions {
    /// Full connection string (`mongodb://…` or `mongodb+srv://…` when the
    /// `dns-resolver` feature is enabled).
    pub uri: String,
    /// Optional application name. Reported to the server on handshake and
    /// visible in `db.currentOp()` and MongoDB Atlas dashboards.
    pub app_name: Option<String>,
    /// Per-operation server-selection timeout. Default: 30 s (driver default).
    pub server_selection_timeout: Option<Duration>,
    /// Per-operation connect timeout. Default: 10 s (driver default).
    pub connect_timeout: Option<Duration>,
    /// Force direct-connection mode (skip replica-set discovery). Useful
    /// against single-node test instances where auto-discovery would fail.
    pub direct_connection: bool,
    /// Default database name. `MongoService::default_database()` reads this
    /// so callers don't have to thread the db name through every call.
    pub default_database: Option<String>,
}

impl MongoOptions {
    /// Start a builder from a connection URI.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            app_name: None,
            server_selection_timeout: None,
            connect_timeout: None,
            direct_connection: false,
            default_database: None,
        }
    }

    /// Set the application name reported to the server.
    pub fn app_name(mut self, name: impl Into<String>) -> Self {
        self.app_name = Some(name.into());
        self
    }

    /// Set the server-selection timeout.
    pub fn server_selection_timeout(mut self, d: Duration) -> Self {
        self.server_selection_timeout = Some(d);
        self
    }

    /// Set the connect timeout.
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = Some(d);
        self
    }

    /// Force direct-connection mode (skip replica-set discovery).
    pub fn direct_connection(mut self, on: bool) -> Self {
        self.direct_connection = on;
        self
    }

    /// Set the default database name returned by
    /// [`MongoService::default_database`].
    pub fn default_database(mut self, name: impl Into<String>) -> Self {
        self.default_database = Some(name.into());
        self
    }

    /// Build a fully-resolved `mongodb::options::ClientOptions`. The driver's
    /// own builder handles URI parsing, TLS detection, and credential
    /// extraction; we layer on the nestrs-only overrides (timeouts,
    /// direct-connection, app name).
    pub async fn into_client_options(self) -> Result<ClientOptions> {
        let mut opts = ClientOptions::parse(&self.uri)
            .await
            .map_err(MongoError::from)?;
        if let Some(name) = self.app_name {
            opts.app_name = Some(name);
        }
        if let Some(d) = self.server_selection_timeout {
            opts.server_selection_timeout = Some(d);
        }
        if let Some(d) = self.connect_timeout {
            opts.connect_timeout = Some(d);
        }
        if self.direct_connection {
            opts.direct_connection = Some(true);
        }
        Ok(opts)
    }
}

/// Global connection URI set by `MongoModule::for_root`. Mirrors the
/// umbrella's pre-extraction behavior (`nestrs::MongoModule::for_root` used
/// to use a `OnceLock<String>` for the URI and a `OnceCell<Client>` for the
/// resolved driver client).
static MONGO_OPTIONS: OnceLock<MongoOptions> = OnceLock::new();
static MONGO_CLIENT: OnceCell<Result<Arc<Client>>> = OnceCell::const_new();

async fn ensure_client() -> Result<Arc<Client>> {
    let cell = MONGO_CLIENT
        .get_or_try_init(|| async {
            let opts = MONGO_OPTIONS
                .get()
                .cloned()
                .ok_or(MongoError::NotConfigured)?;
            let co = opts.into_client_options().await?;
            Client::with_options(co)
                .map(Arc::new)
                .map_err(MongoError::from)
        })
        .await?;
    cell.clone().map_err(|e| match e {
        // Avoid double-wrapping: the OnceCell stores the Err directly.
        MongoError::Driver(e) => MongoError::Driver(e),
        other => other,
    })
}

/// Injectable MongoDB service.
///
/// Resolve it through the DI container the same way the umbrella's old
/// `MongoService` worked: `DynamicModule::from_module::<nestrs::MongoModule>()`
/// inside `#[module(imports = …)]` makes it available. For the new typed
/// repository surface, prefer [`crate::repository::MongoRepository`] which
/// wraps a typed `Collection<T>` and gives you a real `find / save / delete`
/// API.
#[derive(Clone, Default)]
pub struct MongoService;

impl MongoService {
    /// Construct a service handle. The handle is cheap (it's a unit
    /// wrapper); the heavy lifting lives in the `Arc<Client>` singleton
    /// built by [`ensure_client`].
    pub fn new() -> Self {
        Self
    }

    /// Resolve a clone of the underlying driver client.
    pub async fn client(&self) -> Result<Client> {
        Ok(ensure_client().await?.as_ref().clone())
    }

    /// Get a handle to a named database on the resolved client.
    pub async fn database(&self, name: &str) -> Result<Database> {
        Ok(self.client().await?.database(name))
    }

    /// Resolve the default database name from the [`MongoOptions`] set at
    /// `for_root` time. Returns `None` if no default was configured.
    pub fn default_database_name(&self) -> Option<String> {
        MONGO_OPTIONS.get().and_then(|o| o.default_database.clone())
    }

    /// Convenience: resolve the default database (if one was configured) and
    /// surface a clear error otherwise.
    pub async fn default_database(&self) -> Result<Database> {
        let name = self
            .default_database_name()
            .ok_or_else(|| MongoError::InvalidArgument(
                "no default database configured on MongoOptions".into(),
            ))?;
        self.database(&name).await
    }

    /// Run a `ping` against the `admin` database. Returns `Ok(())` on a
    /// successful round-trip; surfaces driver errors as `MongoError::Driver`.
    pub async fn ping(&self) -> Result<()> {
        let c = ensure_client().await?;
        c.list_database_names()
            .await
            .map_err(MongoError::from)?;
        Ok(())
    }

    /// List the database names visible to the connected client.
    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let c = ensure_client().await?;
        c.list_database_names().await.map_err(MongoError::from)
    }
}

impl std::fmt::Debug for MongoService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoService").finish()
    }
}

/// The static module that owns the connection URI. Mirrors the umbrella's
/// pre-extraction `MongoModule` shape: `MongoModule::for_root(uri)` is the
/// pre-app config step, `MongoModule` itself is what you list under
/// `#[module(imports = …)]`.
///
/// Phase E will add `for_root_async` (reads the URI from a
/// `ConfigService` snapshot) and `for_feature` (registers typed
/// `MongoRepository<T>` instances for DI).
pub struct MongoModule;

impl MongoModule {
    /// Static URI setter. Must be called exactly once, before
    /// `NestFactory::create`.
    ///
    /// ```ignore
    /// fn main() {
    ///     nestrs_mongodb::MongoModule::for_root("mongodb://127.0.0.1:27017");
    ///     NestFactory::create::<AppModule>().listen(3000).await;
    /// }
    /// ```
    pub fn for_root(uri: impl Into<String>) -> Self {
        let opts = MongoOptions::new(uri);
        // Ignore `Err(AlreadySet)` — the second `for_root` call is a
        // programming error surfaced as a panic in the legacy umbrella
        // version; we keep the same shape so existing apps don't break.
        let _ = MONGO_OPTIONS.set(opts);
        Self
    }

    /// Configuration-driven variant: takes a fully-built [`MongoOptions`]
    /// (typically constructed from a `ConfigService` snapshot). Like
    /// [`for_root`], must be called once before the app starts.
    pub fn for_root_with_options(opts: MongoOptions) -> Self {
        let _ = MONGO_OPTIONS.set(opts);
        Self
    }
}

impl std::fmt::Debug for MongoModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoModule").finish()
    }
}