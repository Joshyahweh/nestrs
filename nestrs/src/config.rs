use crate::core::{Injectable, Module, ProviderRegistry};
use axum::Router;
use serde::de::DeserializeOwned;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;
use validator::Validate;

#[derive(Debug, Clone)]
pub struct ConfigError {
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ConfigError {}

fn current_env() -> String {
    std::env::var("NESTRS_ENV")
        .or_else(|_| std::env::var("RUST_ENV"))
        .unwrap_or_else(|_| "development".to_string())
}

/// Load typed config from environment variables (optionally `.env` / `.env.<env>` in non-production),
/// then run `validator::Validate`.
pub fn load_config<T>() -> Result<T, ConfigError>
where
    T: DeserializeOwned + Validate,
{
    let env = current_env();
    if env != "production" {
        let _ = dotenvy::dotenv();
        let _ = dotenvy::from_filename(format!(".env.{env}"));
    }

    let cfg = envy::from_env::<T>().map_err(|e| ConfigError {
        message: format!("config env decode error: {e}"),
    })?;

    cfg.validate().map_err(|e| ConfigError {
        message: format!("config validation error: {e}"),
    })?;

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Namespaced configuration (`NESTRS_<NS>__<KEY>`)
//
// Each registered config type declares a namespace (via `#[config(namespace
// = "db")]` or a manual `ConfigNamespace` impl). Values are read from an
// env *overlay* — dotenvy cascade files + the process environment merged
// last — so the whole pipeline is testable without mutating process env.
// ---------------------------------------------------------------------------

/// Marker trait linking a config struct to its `NESTRS_<NS>__<KEY>` namespace.
///
/// Prefer deriving it with `#[config(namespace = "db")]` (nestrs-macros) on
/// top of `#[derive(serde::Deserialize, validator::Validate)]`.
pub trait ConfigNamespace {
    const NAMESPACE: &'static str;
}

/// Default variable prefix, overridable with `NESTRS_ENV_PREFIX`.
pub const DEFAULT_CONFIG_PREFIX: &str = "NESTRS_";

/// Merge one dotenvy file into `map`, insert-if-absent (earliest file in the
/// cascade therefore has the *lowest* precedence: `.env.local` wins over
/// `.env`). Missing files are skipped silently, matching `dotenvy::dotenv`.
fn merge_dotenv_file(map: &mut HashMap<String, String>, path: &Path) {
    let Ok(iter) = dotenvy::from_path_iter(path) else {
        return;
    };
    for item in iter.flatten() {
        let (key, value) = item;
        // Last-write-wins per file: callers control precedence by passing
        // higher-precedence files later. Inserting (not `or_insert`)
        // matches the documented cascade where local > env > base.
        map.insert(key, value);
    }
}

/// Build the env overlay: `.env` → `.env.{env}` → `.env.{env}.local`
/// (lowest → higher precedence), then `process_env` merged last (highest).
///
/// Unlike `dotenvy::dotenv`, this never mutates the process environment —
/// the returned map *is* the environment, which keeps the config pipeline
/// deterministic under parallel tests.
pub fn build_overlay(
    env: &str,
    dir: Option<&Path>,
    process_env: impl IntoIterator<Item = (String, String)>,
) -> Result<HashMap<String, String>, ConfigError> {
    let mut map = HashMap::new();
    let dir = dir.unwrap_or_else(|| Path::new("."));
    if env != "production" {
        merge_dotenv_file(&mut map, &dir.join(".env"));
        merge_dotenv_file(&mut map, &dir.join(format!(".env.{env}")));
        merge_dotenv_file(&mut map, &dir.join(format!(".env.{env}.local")));
    }
    for (k, v) in process_env {
        map.insert(k, v);
    }
    Ok(map)
}

/// [`build_overlay`] against the real process environment.
pub fn resolve_env_overlay(
    env: &str,
    dir: Option<&Path>,
) -> Result<HashMap<String, String>, ConfigError> {
    build_overlay(env, dir, std::env::vars())
}

/// Decode `T` from the overlay using `{prefix}{namespace}__<KEY>` keys.
///
/// The prefix (default `NESTRS_`, override with `NESTRS_ENV_PREFIX`) plus the
/// namespace and the `__` separator are stripped, and the remainder is fed to
/// serde with case-insensitive field matching (e.g. `NESTRS_DB__HOST` →
/// `DatabaseConfig::host`). Keys are matched per namespace, so overlapping
/// field names across namespaces never collide.
pub fn parse_namespaced<T>(
    overlay: &HashMap<String, String>,
    prefix: &str,
    namespace: &str,
) -> Result<T, ConfigError>
where
    T: DeserializeOwned,
{
    let stem = format!("{}{}__", prefix.to_uppercase(), namespace.to_uppercase());
    let filtered: Vec<(String, String)> = overlay
        .iter()
        .filter_map(|(k, v)| {
            let upper = k.to_uppercase();
            upper
                .strip_prefix(&stem)
                .map(|rest| (rest.to_string(), v.clone()))
        })
        .collect();

    envy::from_iter(filtered).map_err(|e| ConfigError {
        message: format!("config decode error for namespace `{namespace}`: {e}"),
    })
}

fn config_prefix() -> String {
    std::env::var("NESTRS_ENV_PREFIX").unwrap_or_else(|_| DEFAULT_CONFIG_PREFIX.to_string())
}

type ParseFn = Box<
    dyn Fn(&HashMap<String, String>, &str) -> Result<Arc<dyn Any + Send + Sync>, ConfigError>
        + Send
        + Sync,
>;

/// A registered namespaced config type; produced by [`Config::register`].
pub struct NamespacedConfig {
    type_id: TypeId,
    namespace: &'static str,
    parse: ParseFn,
}

/// Entry point for registering config types with [`ConfigModule::for_root`].
pub struct Config;

impl Config {
    /// Register the config type `T` (namespace from its [`ConfigNamespace`]
    /// impl). Values are parsed + validated when [`ConfigModule::for_root`]
    /// builds the module.
    pub fn register<T>() -> NamespacedConfig
    where
        T: DeserializeOwned + Validate + ConfigNamespace + Send + Sync + 'static,
    {
        NamespacedConfig {
            type_id: TypeId::of::<T>(),
            namespace: T::NAMESPACE,
            parse: Box::new(|overlay, prefix| {
                let cfg = parse_namespaced::<T>(overlay, prefix, T::NAMESPACE)?;
                cfg.validate().map_err(|e| ConfigError {
                    message: format!(
                        "config validation error for namespace `{}`: {e}",
                        T::NAMESPACE
                    ),
                })?;
                Ok(Arc::new(cfg) as Arc<dyn Any + Send + Sync>)
            }),
        }
    }
}

/// Typed, validated, namespaced configuration values (Nest `ConfigService`).
#[derive(Debug, Clone, Default)]
pub struct ConfigService {
    values: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    namespaces: HashMap<TypeId, &'static str>,
}

impl ConfigService {
    /// Parse + validate every registered config type against `overlay`.
    /// Fails on the first decode or validation error (boot-time rejection).
    pub fn build(
        entries: &[NamespacedConfig],
        overlay: &HashMap<String, String>,
    ) -> Result<Self, ConfigError> {
        Self::build_with_prefix(entries, overlay, &config_prefix())
    }

    /// Like [`ConfigService::build`] with an explicit prefix (test seam).
    pub fn build_with_prefix(
        entries: &[NamespacedConfig],
        overlay: &HashMap<String, String>,
        prefix: &str,
    ) -> Result<Self, ConfigError> {
        let mut values = HashMap::new();
        let mut namespaces = HashMap::new();
        for entry in entries {
            let value = (entry.parse)(overlay, prefix)?;
            values.insert(entry.type_id, value);
            namespaces.insert(entry.type_id, entry.namespace);
        }
        Ok(Self { values, namespaces })
    }

    /// Typed accessor: returns a clone of the stored `Arc<T>`.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Arc<T>, ConfigError> {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|v| v.clone().downcast::<T>().ok())
            .ok_or_else(|| ConfigError {
                message: format!(
                    "config for `{}` was not registered (is it in ConfigModule::for_root?)",
                    std::any::type_name::<T>()
                ),
            })
    }

    /// The namespace `T` was registered under, if it was registered.
    pub fn namespace_of<T: 'static>(&self) -> Option<&'static str> {
        self.namespaces.get(&TypeId::of::<T>()).copied()
    }
}

// Default Injectable builds an *empty* service; `ConfigModule::for_root`
// overrides the provider with the fully parsed one (CacheService precedent).
#[nestrs::async_trait]
impl Injectable for ConfigService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::default())
    }
}

/// Nest-like module for typed config providers (single type).
///
/// Usage:
/// - Define your config struct: `#[derive(serde::Deserialize, validator::Validate, nestrs::NestConfig)] struct AppConfig { ... }`
/// - Import it: `#[module(imports = [nestrs::TypedConfigModule::<AppConfig>], ...)]`
pub struct TypedConfigModule<T>(PhantomData<T>);

impl<T> Default for TypedConfigModule<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T> Module for TypedConfigModule<T>
where
    T: Injectable + Send + Sync + 'static,
{
    fn build() -> (ProviderRegistry, Router) {
        let mut registry = ProviderRegistry::new();
        registry.register::<T>();
        (registry, Router::new())
    }

    fn exports() -> Vec<TypeId> {
        vec![TypeId::of::<T>()]
    }
}

impl<T> crate::core::ModuleGraph for TypedConfigModule<T>
where
    T: Injectable + Send + Sync + 'static,
{
    fn register_providers(registry: &mut ProviderRegistry) {
        registry.register::<T>();
    }

    fn register_controllers(router: Router, _registry: &ProviderRegistry) -> Router {
        router
    }
}

/// Back-compat note: the generic single-type module was renamed to
/// [`TypedConfigModule`] so the non-generic [`ConfigModule`] can host the
/// namespaced config system (`for_root`).
///
/// Namespaced config module: parses + validates every registered config
/// type from the environment at build time and exports a [`ConfigService`].
///
/// ```ignore
/// #[config(namespace = "db")]
/// #[derive(serde::Deserialize, validator::Validate)]
/// struct DatabaseConfig { #[validate(length(min = 1))] host: String }
///
/// #[module(
///     imports = [ConfigModule::for_root(vec![
///         Config::<DatabaseConfig>::register(),
///         Config::<AuthConfig>::register(),
///     ])],
/// )]
/// struct AppModule;
/// // anywhere: registry.get::<ConfigService>().get::<DatabaseConfig>()
/// ```
pub struct ConfigModule;

impl ConfigModule {
    /// Register each config type, parse its `NESTRS_<NS>__*` values from the
    /// env overlay (dotenvy cascade + process env), validate, and export the
    /// resulting [`ConfigService`]. Panics on invalid env (boot-time
    /// rejection, matching the `NestConfig` derive's panic-on-load).
    pub fn for_root(entries: Vec<NamespacedConfig>) -> crate::core::DynamicModule {
        let env = current_env();
        let overlay = resolve_env_overlay(&env, None)
            .unwrap_or_else(|e| panic!("config overlay failed: {e}"));
        let service = ConfigService::build(&entries, &overlay)
            .unwrap_or_else(|e| panic!("ConfigModule::for_root failed: {e}"));

        let mut registry = ProviderRegistry::new();
        registry.override_provider::<ConfigService>(std::sync::Arc::new(service));
        crate::core::DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<ConfigService>()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, Validate)]
    struct DbConfig {
        #[validate(length(min = 1))]
        host: String,
        #[serde(default = "default_port")]
        port: u16,
    }
    fn default_port() -> u16 {
        5432
    }

    #[derive(Debug, Deserialize, Validate)]
    struct AuthConfig {
        #[validate(length(min = 1))]
        name: String,
    }

    impl ConfigNamespace for DbConfig {
        const NAMESPACE: &'static str = "db";
    }
    impl ConfigNamespace for AuthConfig {
        const NAMESPACE: &'static str = "auth";
    }

    fn overlay(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parse_namespaced_maps_prefixed_keys_to_fields() {
        let o = overlay(&[
            ("NESTRS_DB__HOST", "localhost"),
            ("NESTRS_DB__PORT", "6543"),
            // Another namespace's key must not leak into the `db` decode.
            ("NESTRS_AUTH__NAME", "authsvc"),
        ]);
        let db: DbConfig = parse_namespaced(&o, "NESTRS_", "db").expect("decode db");
        assert_eq!(db.host, "localhost");
        assert_eq!(db.port, 6543, "explicit value beats serde default");

        let auth: AuthConfig = parse_namespaced(&o, "NESTRS_", "auth").expect("decode auth");
        assert_eq!(auth.name, "authsvc", "NESTRS_AUTH__NAME maps to auth.name");
    }

    #[test]
    fn parse_namespaced_applies_serde_defaults() {
        let o = overlay(&[("NESTRS_DB__HOST", "h")]);
        let db: DbConfig = parse_namespaced::<DbConfig>(&o, "NESTRS_", "db").expect("decode");
        assert_eq!(db.port, 5432);
    }

    #[test]
    fn parse_namespaced_honors_prefix_override() {
        let o = overlay(&[("APP_DB__HOST", "via-prefix")]);
        let db: DbConfig = parse_namespaced::<DbConfig>(&o, "APP_", "db").expect("decode");
        assert_eq!(db.host, "via-prefix");
        assert!(parse_namespaced::<DbConfig>(&o, "NESTRS_", "db").is_err());
    }

    #[test]
    fn build_overlay_cascade_local_overrides_env_overrides_base() {
        let dir = std::env::temp_dir().join(format!("nestrs-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join(".env"),
            "NESTRS_DB__HOST=base\nNESTRS_DB__PORT=1\n",
        )
        .expect("write .env");
        std::fs::write(dir.join(".env.test"), "NESTRS_DB__PORT=2").expect("write .env.test");
        std::fs::write(
            dir.join(".env.test.local"),
            "NESTRS_DB__HOST=local\nNESTRS_DB__PORT=3",
        )
        .expect("write .env.test.local");

        let o = build_overlay("test", Some(&dir), std::iter::empty()).expect("overlay");
        assert_eq!(o.get("NESTRS_DB__HOST").map(String::as_str), Some("local"));
        assert_eq!(o.get("NESTRS_DB__PORT").map(String::as_str), Some("3"));
    }

    #[test]
    fn build_overlay_process_env_wins_over_files() {
        let dir = std::env::temp_dir().join(format!("nestrs-cfg-{}-p", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(".env"), "NESTRS_DB__HOST=file").expect("write .env");
        let o = build_overlay(
            "test",
            Some(&dir),
            vec![("NESTRS_DB__HOST".to_string(), "env".to_string())],
        )
        .expect("overlay");
        assert_eq!(o.get("NESTRS_DB__HOST").map(String::as_str), Some("env"));
    }

    #[test]
    fn config_service_build_validates_and_serves_typed_values() {
        let o = overlay(&[
            ("NESTRS_DB__HOST", "localhost"),
            ("NESTRS_AUTH__NAME", "authsvc"),
        ]);
        let svc = ConfigService::build_with_prefix(
            &[
                Config::register::<DbConfig>(),
                Config::register::<AuthConfig>(),
            ],
            &o,
            "NESTRS_",
        )
        .expect("build");
        let db = svc.get::<DbConfig>().expect("typed get");
        assert_eq!(db.host, "localhost");
        assert_eq!(svc.namespace_of::<DbConfig>(), Some("db"));
        assert_eq!(svc.namespace_of::<AuthConfig>(), Some("auth"));
        // Unregistered type errors.
        struct Unregistered;
        assert!(svc.get::<Unregistered>().is_err());
    }

    #[test]
    fn config_service_rejects_invalid_env_at_boot() {
        let o = overlay(&[("NESTRS_DB__HOST", ""), ("NESTRS_AUTH__NAME", "ok")]);
        let err = ConfigService::build_with_prefix(
            &[
                Config::register::<DbConfig>(),
                Config::register::<AuthConfig>(),
            ],
            &o,
            "NESTRS_",
        )
        .expect_err("host empty => validation error");
        assert!(
            err.message.contains("db"),
            "error names the namespace: {err}"
        );
    }
}
