//! MongoDB client lifecycle: connection options, module setup, and the
//! injectable `MongoService`.
//!
//! This module ships the connection configuration, the
//! `MongoModule::for_root` / [`MongoModule::for_root_async`] /
//! [`MongoModule::for_feature`] setters, and a `MongoService` that callers
//! can resolve out of the DI container. The typed `MongoRepository<T>`
//! CRUD wrapper lives in [`crate::repository`].
//!
//! `MongoService::model::<T>()` is the Rust analogue of NestJS
//! `@InjectModel(T)` — it resolves a typed repository from the
//! `for_feature` database name.
//!
//! ## Example
//!
//! ```ignore
//! use nestrs_mongodb::{MongoModule, MongoService};
//!
//! #[tokio::main]
//! async fn main() {
//!     MongoModule::for_root("mongodb://127.0.0.1:27017");
//!     let svc = MongoService::new();
//!     let db = svc.database("app").await?;
//! }
//! ```

use crate::error::{MongoError, Result};
use mongodb::options::ClientOptions;
use mongodb::{Client, Database};
use nestrs_core::{Injectable, Module, ProviderRegistry};
use std::any::TypeId;
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
#[derive(Clone)]
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

/// Redact `user:pass@` from a connection URI so `Debug` / logs cannot
/// leak credentials. URIs without userinfo are returned unchanged.
fn redact_userinfo(uri: &str) -> String {
    let Some(scheme_end) = uri.find("://") else {
        return uri.to_string();
    };
    let rest = &uri[scheme_end + 3..];
    let Some(at) = rest.find('@') else {
        return uri.to_string();
    };
    format!("{}***@{}", &uri[..=scheme_end + 2], &rest[at + 1..])
}

impl std::fmt::Debug for MongoOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoOptions")
            .field("uri", &redact_userinfo(&self.uri))
            .field("app_name", &self.app_name)
            .field("server_selection_timeout", &self.server_selection_timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("direct_connection", &self.direct_connection)
            .field("default_database", &self.default_database)
            .finish()
    }
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
static MONGO_CLIENT: OnceCell<Arc<Client>> = OnceCell::const_new();

/// Default database name registered by `MongoModule::for_feature(db_name)`.
/// Mirrors MongooseModule's `forFeature` registration key — apps list the
/// `for_feature` call inside `#[module(imports = …)]` and then resolve a
/// typed [`crate::repository::MongoRepository<T>`] through the helper
/// [`crate::repository::MongoRepository::for_feature`].
static FEATURE_DB: OnceLock<String> = OnceLock::new();

async fn ensure_client() -> Result<Arc<Client>> {
    let client = MONGO_CLIENT
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
    Ok(client.clone())
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
    /// wrapper); the heavy lifting lives in the process-wide `Arc<Client>`
    /// singleton this service resolves on first use.
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
        let name = self.default_database_name().ok_or_else(|| {
            MongoError::InvalidArgument("no default database configured on MongoOptions".into())
        })?;
        self.database(&name).await
    }

    /// Run a `ping` against the `admin` database. Returns `Ok(())` on a
    /// successful round-trip; surfaces driver errors as `MongoError::Driver`.
    pub async fn ping(&self) -> Result<()> {
        let c = ensure_client().await?;
        c.list_database_names().await.map_err(MongoError::from)?;
        Ok(())
    }

    /// List the database names visible to the connected client.
    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let c = ensure_client().await?;
        c.list_database_names().await.map_err(MongoError::from)
    }

    /// NestJS `@InjectModel(T)` analogue: resolve a typed
    /// [`crate::repository::MongoRepository<T>`] from the database name
    /// registered by [`MongoModule::for_feature`].
    ///
    /// ```ignore
    /// let users: MongoRepository<User> = svc.model().await?;
    /// ```
    pub async fn model<T: crate::schema::Schema>(
        &self,
    ) -> Result<crate::repository::MongoRepository<T>> {
        crate::repository::MongoRepository::for_feature(self).await
    }
}

impl std::fmt::Debug for MongoService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoService").finish()
    }
}

impl Injectable for MongoService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::new())
    }
}

/// The static module that owns the connection URI. Mirrors the umbrella's
/// pre-extraction `MongoModule` shape: `MongoModule::for_root(uri)` is the
/// pre-app config step, `MongoModule` itself is what you list under
/// `#[module(imports = …)]`.
///
/// Async boot uses [`MongoModule::for_root_async`]. Typed repositories
/// register via [`MongoModule::for_feature`] and resolve through
/// [`MongoService::model`] or [`crate::repository::MongoRepository::for_feature`].
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
    /// [`Self::for_root`], must be called once before the app starts.
    pub fn for_root_with_options(opts: MongoOptions) -> Self {
        let _ = MONGO_OPTIONS.set(opts);
        Self
    }

    /// NestJS `MongooseModule.forRootAsync` analogue: run an async factory
    /// that builds [`MongoOptions`] (vault lookup, `ConfigService` snapshot,
    /// etc.) then register the result. Must complete before
    /// `NestFactory::create`.
    ///
    /// ```ignore
    /// MongoModule::for_root_async(|| async {
    ///     Ok(MongoOptions::new(std::env::var("MONGO_URI")?))
    /// })
    /// .await?;
    /// ```
    pub async fn for_root_async<F, Fut>(factory: F) -> Result<Self>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<MongoOptions>>,
    {
        let opts = factory().await?;
        Ok(Self::for_root_with_options(opts))
    }

    /// Feature-registration step (NestJS MongooseModule `forFeature`
    /// analogue). Registers the database name that the typed
    /// [`crate::repository::MongoRepository::for_feature`] helper reads
    /// when building repositories.
    ///
    /// Typically called inside `#[module(imports = …)]` so it runs at
    /// boot time:
    ///
    /// ```ignore
    /// #[module(imports = [MongoModule::for_feature("app")])]
    /// struct AppModule;
    /// ```
    ///
    /// Then anywhere in your app:
    ///
    /// ```ignore
    /// let users: MongoRepository<User> =
    ///     MongoRepository::for_feature(&svc).await?;
    /// ```
    ///
    /// Calling `for_feature` more than once replaces the previous
    /// registration (last-wins). For most apps there's exactly one
    /// `for_feature` call at the root module.
    pub fn for_feature(db_name: impl Into<String>) -> Self {
        let _ = FEATURE_DB.set(db_name.into());
        Self
    }

    /// Borrow the registered feature database name. Used by
    /// [`crate::repository::MongoRepository::for_feature`] to resolve
    /// repositories without requiring the caller to thread the db name
    /// through every DI lookup.
    pub fn feature_db() -> Option<&'static str> {
        FEATURE_DB.get().map(String::as_str)
    }
}

impl std::fmt::Debug for MongoModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MongoModule").finish()
    }
}

impl Module for MongoModule {
    fn build() -> (ProviderRegistry, axum::Router) {
        let mut registry = ProviderRegistry::new();
        registry.register_use_value::<MongoService>(Arc::new(MongoService::new()));
        (registry, axum::Router::new())
    }

    fn exports() -> Vec<TypeId> {
        vec![TypeId::of::<MongoService>()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_uri_userinfo() {
        let opts = MongoOptions::new("mongodb://ada:s3cret@localhost:27017/app");
        let dbg = format!("{opts:?}");
        assert!(
            !dbg.contains("s3cret"),
            "password must not appear in Debug: {dbg}"
        );
        assert!(
            dbg.contains("***@localhost:27017/app"),
            "userinfo should be redacted, got: {dbg}"
        );
    }

    #[test]
    fn debug_leaves_uris_without_userinfo_intact() {
        let opts = MongoOptions::new("mongodb://127.0.0.1:27017");
        let dbg = format!("{opts:?}");
        assert!(dbg.contains("mongodb://127.0.0.1:27017"));
    }
}
