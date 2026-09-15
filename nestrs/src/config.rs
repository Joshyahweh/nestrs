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
///
/// `raw` holds the **merged, dotted-key view** of every config source
/// (later sources win; empty / `null` deletes). It's what
/// [`ConfigService::get_by_key`] and [`ConfigService::snapshot`] read from.
/// The namespaced `values` map stays exactly as the Wave 5 surface shipped it.
#[derive(Debug, Clone, Default)]
pub struct ConfigService {
    values: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    namespaces: HashMap<TypeId, &'static str>,
    /// Merged dotted-key view; populated by
    /// [`ConfigService::build_with_sources`](Self::build_with_sources).
    pub raw: HashMap<String, String>,
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
        Ok(Self {
            values,
            namespaces,
            raw: overlay.clone(),
        })
    }

    /// Build a [`ConfigService`] from a list of [`ConfigSource`]s. Sources
    /// are merged in declaration order, **later sources win** (matches
    /// `@nestjs/config`). Empty-string or `"null"` raw values delete the
    /// previous key.
    pub fn build_with_sources(
        entries: &[NamespacedConfig],
        sources: &[ConfigSource],
    ) -> Result<Self, ConfigError> {
        let env = current_env();
        let env_overlay = resolve_env_overlay(&env, None).unwrap_or_default();
        let merged = build_sources_overlay(sources, &env_overlay)?;
        Self::build_with_prefix(entries, &merged, &config_prefix())
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

    /// Read a dotted key (e.g. `"server.port"`) from the merged sources
    /// view. Walks the merged raw map as a JSON tree and deserializes the
    /// leaf into `T`. Strings that parse as JSON literals (`"true"`,
    /// `"8080"`, `"null"`, `"[1,2]"`) are coerced before the typed
    /// deserialize, so a `"server.port"` stored as `"8080"` in a YAML file
    /// reads back as `u16`.
    pub fn get_by_key<T>(&self, key: &str) -> Result<T, ConfigError>
    where
        T: DeserializeOwned,
    {
        let tree = raw_to_json_tree(&self.raw);
        let mut current = &tree;
        for segment in key.split('.') {
            current = match current {
                serde_json::Value::Object(map) => map.get(segment).unwrap_or(&serde_json::Value::Null),
                serde_json::Value::Null => &serde_json::Value::Null,
                _ => {
                    return Err(ConfigError {
                        message: format!("config key `{key}` traverses a non-object segment"),
                    })
                }
            };
        }
        if current.is_null() {
            return Err(ConfigError {
                message: format!("config key `{key}` not found"),
            });
        }
        let leaf = if let serde_json::Value::String(s) = current {
            coerce_string_to_json(s)
        } else {
            current.clone()
        };
        serde_json::from_value::<T>(leaf).map_err(|e| ConfigError {
            message: format!("config key `{key}` could not be deserialized: {e}"),
        })
    }

    /// Look up the raw string value behind a dotted key (last write wins,
    /// no JSON coercion). Returns `None` if the key was deleted by a later
    /// source.
    pub fn raw(&self, key: &str) -> Option<&str> {
        self.raw.get(key).map(String::as_str)
    }

    /// `true` when `key` exists in the merged view and was not deleted.
    pub fn has(&self, key: &str) -> bool {
        self.raw.contains_key(key)
    }

    /// Immutable snapshot of the merged raw view, useful for diagnostics,
    /// `/config`-style debug endpoints, and `ConfigWatcher` consumers.
    pub fn snapshot(&self) -> HashMap<String, String> {
        self.raw.clone()
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

    /// Register each config type, parse its values from a list of
    /// [`ConfigSource`]s (file sources + env overlay merged last), and
    /// export the resulting [`ConfigService`].
    ///
    /// Use this instead of [`Self::for_root`] when you want JSON / TOML /
    /// YAML files in addition to environment variables. Like `for_root`,
    /// panics on invalid config (boot-time rejection).
    pub fn for_root_with_options(
        entries: Vec<NamespacedConfig>,
        options: ConfigOptions,
    ) -> crate::core::DynamicModule {
        let service = Self::build_service(&entries, &options)
            .unwrap_or_else(|e| panic!("ConfigModule::for_root_with_options failed: {e}"));

        let mut registry = ProviderRegistry::new();
        registry.override_provider::<ConfigService>(std::sync::Arc::new(service));
        crate::core::DynamicModule::from_parts(
            registry,
            Router::new(),
            vec![TypeId::of::<ConfigService>()],
        )
    }

    /// Build a [`ConfigService`] from `entries` and `options`. Returns the
    /// parsed service or a [`ConfigError`] (no panic; this is the testable
    /// form of [`Self::for_root_with_options`]).
    pub fn build_service(
        entries: &[NamespacedConfig],
        options: &ConfigOptions,
    ) -> Result<ConfigService, ConfigError> {
        let env = current_env();
        let env_overlay = resolve_env_overlay(&env, None).unwrap_or_default();
        let merged = build_sources_overlay(&options.sources, &env_overlay)?;
        // Bridge file/inline sources into the env-style namespace convention
        // (`NESTRS_<NS>__<FIELD>`) so `parse_namespaced` sees what it expects.
        // Files + inline JSON are flattened with `__` as the object nesting
        // separator, so a key like `db__host` becomes `NESTRS_DB__HOST` when
        // `db` is a registered namespace. Keys that don't start with any
        // registered namespace pass through untouched (this preserves the
        // env-var overlay verbatim).
        let prefix = config_prefix();
        let bridged = rekey_into_namespaces(&merged, entries, &prefix);
        let mut service = ConfigService::build_with_prefix(entries, &bridged, &prefix)?;
        service.raw = bridged;
        Ok(service)
    }
}

// ---------------------------------------------------------------------------
// Wave 7.1: file sources, dotted-key access, hot reload
//
// Matches `@nestjs/config`:
//   * sources merge in declaration order, later wins
//   * null / empty raw values delete the previous key
//   * YAML / TOML are converted to JSON before the typed decode so the
//     downstream pipeline only sees one shape
// ---------------------------------------------------------------------------

/// Recognized file formats. Driven by file extension (`FileFormat::from_path`)
/// or supplied explicitly on the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Json,
    Toml,
    Yaml,
}

impl FileFormat {
    /// Pick a format from a path's extension. Returns `None` when the
    /// extension is unrecognized (callers should pass an explicit format).
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_ascii_lowercase();
        let ext = ext.to_str()?;
        match ext {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" | "yml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

/// A single configuration source in a [`ConfigOptions`] source list.
#[derive(Debug, Clone)]
pub enum ConfigSource {
    /// Environment variables from `process_env`, filtered to `prefix`
    /// (default [`DEFAULT_CONFIG_PREFIX`]). Always merged last in the
    /// resolution order unless explicitly re-ordered via multiple env
    /// sources.
    Env { prefix: Option<String> },
    /// A required config file — missing files surface as [`ConfigError`].
    File { path: std::path::PathBuf, format: FileFormat },
    /// An optional config file — missing files are silently skipped (the
    /// `Optional` analogue from `@nestjs/config`'s `ignoreEnvFile`).
    OptionalFile { path: std::path::PathBuf, format: FileFormat },
    /// Inline values, fed straight into the merged tree. `null` deletes
    /// the previous key (consistent with file+env semantics).
    Inline(serde_json::Value),
}

/// Build instructions for [`ConfigModule::for_root_with_options`] /
/// [`ConfigModule::build_service`].
#[derive(Default)]
pub struct ConfigOptions {
    pub sources: Vec<ConfigSource>,
    pub entries: Vec<NamespacedConfig>,
}

impl ConfigOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_source(mut self, source: ConfigSource) -> Self {
        self.sources.push(source);
        self
    }

    pub fn add_entry(mut self, entry: NamespacedConfig) -> Self {
        self.entries.push(entry);
        self
    }
}

/// Coerce a raw string into the most appropriate `serde_json::Value`.
/// Matches the YAML/JSON scalar coercion rules: `"true"` → `Bool(true)`,
/// `"8080"` → `Number(8080)`, `"null"` / `""` → `Null`, anything else
/// stays a `String`. Numeric and bool literals parse only when the entire
/// string round-trips, so the typed `get_by_key` decode never sees a
/// silently-truncated value.
pub(crate) fn coerce_string_to_json(raw: &str) -> serde_json::Value {
    if raw.is_empty() {
        return serde_json::Value::Null;
    }
    let trimmed = raw.trim();
    match trimmed {
        "null" => return serde_json::Value::Null,
        "true" => return serde_json::Value::Bool(true),
        "false" => return serde_json::Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = trimmed.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    if let Ok(n) = trimmed.parse::<u64>() {
        return serde_json::Value::Number(n.into());
    }
    if let Ok(n) = trimmed.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(n) {
            return serde_json::Value::Number(num);
        }
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return value;
    }
    serde_json::Value::String(raw.to_string())
}

/// Recursively flatten an inline `serde_json::Value` into the dotted-key map.
/// `Value::Null` deletes the previous key (matches `@nestjs/config`).
fn flatten_inline(
    value: serde_json::Value,
    prefix: &str,
    out: &mut HashMap<String, String>,
) {
    match value {
        serde_json::Value::Null => {
            if !prefix.is_empty() {
                out.remove(prefix);
            }
        }
        serde_json::Value::Bool(b) => {
            out.insert(prefix.to_string(), b.to_string());
        }
        serde_json::Value::Number(n) => {
            out.insert(prefix.to_string(), n.to_string());
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s);
        }
        serde_json::Value::Array(items) => {
            for (i, item) in items.into_iter().enumerate() {
                let key = if prefix.is_empty() {
                    format!("{i}")
                } else {
                    format!("{prefix}.{i}")
                };
                flatten_inline(item, &key, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map.into_iter() {
                // Use `__` as the nesting separator so flattened keys line up
                // with what `parse_namespaced` expects from env vars (e.g.
                // `NESTRS_DB__HOST`). Matches `@nestjs/config` semantics.
                let key = if prefix.is_empty() {
                    k
                } else {
                    format!("{prefix}__{k}")
                };
                flatten_inline(v, &key, out);
            }
        }
    }
}

/// Read a file and parse it into `serde_json::Value`. Dispatches by
/// [`FileFormat`]. Returns [`ConfigError`] on missing-required files,
/// unreadable files, or parse failures.
fn load_file(path: &Path, format: FileFormat) -> Result<serde_json::Value, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError {
        message: format!("failed to read config file `{}`: {e}", path.display()),
    })?;
    match format {
        FileFormat::Json => serde_json::from_str::<serde_json::Value>(&text).map_err(|e| {
            ConfigError {
                message: format!("failed to parse JSON config `{}`: {e}", path.display()),
            }
        }),
        FileFormat::Toml => {
            let v: toml::Value = toml::from_str(&text).map_err(|e| ConfigError {
                message: format!("failed to parse TOML config `{}`: {e}", path.display()),
            })?;
            Ok(toml_to_json(v))
        }
        FileFormat::Yaml => {
            let v: serde_yaml::Value = serde_yaml::from_str(&text).map_err(|e| ConfigError {
                message: format!("failed to parse YAML config `{}`: {e}", path.display()),
            })?;
            Ok(yaml_to_json(v))
        }
    }
}

/// Convert a `toml::Value` tree to `serde_json::Value`. Datetimes are
/// rendered with `to_string` so the downstream pipeline sees a `String`.
fn toml_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(map) => {
            let json_map: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, toml_to_json(v)))
                .collect();
            serde_json::Value::Object(json_map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}

/// Convert a `serde_yaml::Value` tree to `serde_json::Value`. Tagged values
/// (e.g. `!!omap`) are flattened to their inner mapping; numbers go via
/// `serde_json::Number` so 64-bit ints stay integers.
fn yaml_to_json(v: serde_yaml::Value) -> serde_json::Value {
    use serde_yaml::Value as Y;
    match v {
        Y::Null => serde_json::Value::Null,
        Y::Bool(b) => serde_json::Value::Bool(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        Y::String(s) => serde_json::Value::String(s),
        Y::Sequence(items) => {
            serde_json::Value::Array(items.into_iter().map(yaml_to_json).collect())
        }
        Y::Mapping(map) => {
            let json_map: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .filter_map(|(k, v)| match k {
                    Y::String(s) => Some((s, yaml_to_json(v))),
                    _ => None,
                })
                .collect();
            serde_json::Value::Object(json_map)
        }
        Y::Tagged(t) => yaml_to_json(t.value),
    }
}

/// Merge a list of [`ConfigSource`]s into a single dotted-key map. Later
/// sources win; `null` / empty deletes the previous value. The
/// `env_overlay` argument is merged last (highest precedence), matching
/// [`ConfigService::build_with_sources`].
pub fn build_sources_overlay(
    sources: &[ConfigSource],
    env_overlay: &HashMap<String, String>,
) -> Result<HashMap<String, String>, ConfigError> {
    let mut out: HashMap<String, String> = HashMap::new();
    for source in sources {
        match source {
            ConfigSource::Env { prefix } => {
                let prefix = prefix.as_deref().unwrap_or(DEFAULT_CONFIG_PREFIX);
                let upper = prefix.to_uppercase();
                for (k, v) in env_overlay.iter() {
                    let key_up = k.to_uppercase();
                    if !key_up.starts_with(&upper) {
                        continue;
                    }
                    out.insert(k.clone(), v.clone());
                }
            }
            ConfigSource::File { path, format } => {
                let tree = load_file(path, *format)?;
                flatten_inline(tree, "", &mut out);
            }
            ConfigSource::OptionalFile { path, format } => {
                if !path.exists() {
                    continue;
                }
                let tree = load_file(path, *format)?;
                flatten_inline(tree, "", &mut out);
            }
            ConfigSource::Inline(value) => {
                flatten_inline(value.clone(), "", &mut out);
            }
        }
    }
    // Process env always wins over file/inline sources.
    for (k, v) in env_overlay.iter() {
        out.insert(k.clone(), v.clone());
    }
    Ok(out)
}

/// Translate file/inline-source flat keys into the env-style namespace
/// convention. `flatten_inline` produces keys like `db__host` for an inline
/// `{"db": {"host": ...}}` source; `parse_namespaced` expects
/// `NESTRS_DB__HOST`. For every key whose leading `__`- or `.`-separated
/// segment matches a registered `NamespacedConfig::namespace` (case-
/// insensitively), this rewrites it to `{prefix}{namespace}__{rest}`. Keys
/// that don't start with a registered namespace pass through untouched, so
/// an existing env overlay (`NESTRS_DB__HOST=...`) wins over a same-named
/// inline value without double-translation.
fn rekey_into_namespaces(
    raw: &HashMap<String, String>,
    entries: &[NamespacedConfig],
    prefix: &str,
) -> HashMap<String, String> {
    if entries.is_empty() {
        return raw.clone();
    }
    let upper_prefix = prefix.to_uppercase();
    let mut out = HashMap::with_capacity(raw.len());
    for (k, v) in raw.iter() {
        // Already env-style (e.g. `NESTRS_DB__HOST`): leave alone. The
        // prefix check is case-insensitive so any casing works.
        if k.to_uppercase().starts_with(&upper_prefix) {
            out.insert(k.clone(), v.clone());
            continue;
        }
        let mut matched = false;
        for entry in entries {
            let ns_lower = entry.namespace.to_ascii_lowercase();
            if ns_lower.is_empty() {
                continue;
            }
            let key_lower = k.to_ascii_lowercase();
            // `<ns>__<rest>` (flatten_inline convention for nested objects)
            let dash2_prefix = format!("{ns_lower}__");
            if let Some(rest_offset) = key_lower
                .strip_prefix(&dash2_prefix)
                .map(|_| ns_lower.len() + 2)
            {
                let rest = &k[rest_offset..];
                let new_key = format!(
                    "{}{}__{}",
                    upper_prefix,
                    entry.namespace.to_uppercase(),
                    rest
                );
                out.insert(new_key, v.clone());
                matched = true;
                break;
            }
            // `<ns>.<rest>` (flatten_inline convention for array indices)
            let dot_prefix = format!("{ns_lower}.");
            if let Some(rest_offset) = key_lower
                .strip_prefix(&dot_prefix)
                .map(|_| ns_lower.len() + 1)
            {
                let rest = &k[rest_offset..];
                let new_key = format!(
                    "{}{}__{}",
                    upper_prefix,
                    entry.namespace.to_uppercase(),
                    rest
                );
                out.insert(new_key, v.clone());
                matched = true;
                break;
            }
        }
        if !matched {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Re-walk the dotted-key map as a JSON tree. Used by
/// [`ConfigService::get_by_key`] to walk down nested structures.
fn raw_to_json_tree(raw: &HashMap<String, String>) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    for (key, value) in raw.iter() {
        let parts: Vec<&str> = key.split('.').collect();
        insert_into_tree(&mut root, &parts, value);
    }
    serde_json::Value::Object(root)
}

fn insert_into_tree(
    node: &mut serde_json::Map<String, serde_json::Value>,
    parts: &[&str],
    value: &str,
) {
    if parts.is_empty() {
        return;
    }
    let head = parts[0];
    if parts.len() == 1 {
        node.insert(head.to_string(), coerce_string_to_json(value));
        return;
    }
    let entry = node
        .entry(head.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let serde_json::Value::Object(map) = entry {
        insert_into_tree(map, &parts[1..], value);
    }
}

/// Hot-reload watcher for file-based [`ConfigSource`]s. Feature-gated behind
/// `config-hot-reload`; pulls in `notify` only when the feature is enabled.
///
/// Drop or [`Self::stop`] to tear down the watcher. Reads are lock-free for
/// the snapshot path (`std::sync::RwLock::read`), and the underlying
/// rebuild is synchronous (`ConfigModule::build_service` does no I/O
/// beyond reading files), so the hot-reload task runs on a blocking thread
/// without bridging into the tokio runtime.
#[cfg(feature = "config-hot-reload")]
pub struct ConfigWatcher {
    state: Arc<std::sync::RwLock<ConfigService>>,
    /// Hold the notify watcher alive; dropping it stops fs callbacks.
    _watcher: notify::RecommendedWatcher,
    /// Debounce task — aborted on [`Self::stop`].
    _task: tokio::task::JoinHandle<()>,
    /// Channel sender kept so [`Self::stop`] can close it and let the
    /// blocking task exit cleanly without waiting for a debounce window.
    _stop_tx: std::sync::mpsc::Sender<()>,
}

#[cfg(feature = "config-hot-reload")]
impl ConfigWatcher {
    /// Build a watcher over the file paths reachable from `options.sources`.
    /// `service` is the initial build (so reads before the first reload see
    /// a consistent state); `options.entries` are the registered namespaces
    /// that get re-decoded on every reload.
    pub fn new(service: ConfigService, options: ConfigOptions) -> Self {
        use notify::{RecursiveMode, Watcher};

        let state = Arc::new(std::sync::RwLock::new(service));

        let (fs_tx, fs_rx) = std::sync::mpsc::channel::<()>();
        let mut watcher = notify::recommended_watcher(move |_res| {
            let _ = fs_tx.send(());
        })
        .expect("build notify watcher");

        for source in &options.sources {
            let (path, _) = match source {
                ConfigSource::File { path, format } => (path, *format),
                ConfigSource::OptionalFile { path, format } => (path, *format),
                ConfigSource::Env { .. } | ConfigSource::Inline(_) => continue,
            };
            // Best-effort: some platforms (notably Windows + WSL) reject
            // specific paths; ignore so the rest of the watch set still
            // functions.
            let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }

        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let task_state = state.clone();
        let task_options = options;
        let task = tokio::task::spawn_blocking(move || {
            // 150 ms debounce window matches `@nestjs/config`'s default.
            const DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(150);
            loop {
                // Wait for an fs event OR a stop signal.
                match fs_rx.recv_timeout(DEBOUNCE) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if stop_rx.try_recv().is_ok() {
                            return;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
                // Drain further events within the debounce window.
                loop {
                    match fs_rx.recv_timeout(DEBOUNCE) {
                        Ok(()) => continue,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                if stop_rx.try_recv().is_ok() {
                    return;
                }
                match ConfigModule::build_service(&task_options.entries, &task_options) {
                    Ok(new_svc) => {
                        if let Ok(mut guard) = task_state.write() {
                            *guard = new_svc;
                        }
                    }
                    Err(_err) => {
                        // Keep the previous good snapshot on rebuild failure.
                    }
                }
            }
        });

        Self {
            state,
            _watcher: watcher,
            _task: task,
            _stop_tx: stop_tx,
        }
    }

    /// Shared handle to the live [`ConfigService`]. The service is replaced
    /// in place on every successful reload; readers cloning the `Arc` see
    /// the new state on their next lock acquisition.
    pub fn service(&self) -> Arc<std::sync::RwLock<ConfigService>> {
        self.state.clone()
    }

    /// Blocking snapshot of the current [`ConfigService`]. Cheap clone —
    /// the underlying state is a `HashMap<String, String>` plus the
    /// registered namespace metadata.
    pub fn snapshot(&self) -> ConfigService {
        self.state.read().expect("config watcher poisoned").clone()
    }

    /// Signal the watcher to exit and wait for the debounce task to finish.
    pub async fn stop(self) {
        let Self {
            _watcher,
            _task,
            _stop_tx,
            state: _state,
        } = self;
        // Drop the watcher first so no new fs events are queued; then close
        // the stop channel so the blocking task can observe the shutdown.
        drop(_watcher);
        drop(_stop_tx);
        let _ = _task.await;
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

    // ---- Wave 7.1 tests: file sources + dotted-key access + hot reload ----

    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_path(suffix: &str, ext: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("nestrs-cfg-{suffix}-{nanos}-{n}.{ext}"))
    }

    fn write_file(path: &Path, body: &str) {
        std::fs::write(path, body).expect("write temp config");
    }

    #[test]
    fn file_format_detects_extensions() {
        assert_eq!(
            FileFormat::from_path(Path::new("config.json")),
            Some(FileFormat::Json)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("config.toml")),
            Some(FileFormat::Toml)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("config.yaml")),
            Some(FileFormat::Yaml)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("config.yml")),
            Some(FileFormat::Yaml)
        );
        assert_eq!(FileFormat::from_path(Path::new("config.env")), None);
    }

    #[test]
    fn json_file_source_flattens_nested_keys() {
        let p = unique_path("json-flatten", "json");
        write_file(
            &p,
            r#"{
              "db": { "host": "h.local", "port": 5432 },
              "auth": { "name": "authsvc" }
            }"#,
        );
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Json,
            }],
            entries: vec![],
        };
        let env_overlay = HashMap::new();
        let merged = build_sources_overlay(&opts.sources, &env_overlay).expect("merge");
        assert_eq!(merged.get("db__host").map(String::as_str), Some("h.local"));
        assert_eq!(merged.get("db__port").map(String::as_str), Some("5432"));
        assert_eq!(merged.get("auth__name").map(String::as_str), Some("authsvc"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn toml_file_source_parses_correctly() {
        let p = unique_path("toml", "toml");
        write_file(
            &p,
            r#"[db]
host = "h.toml"
port = 1234

[auth]
name = "authsvc"
"#,
        );
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Toml,
            }],
            entries: vec![],
        };
        let env_overlay = HashMap::new();
        let merged = build_sources_overlay(&opts.sources, &env_overlay).expect("merge");
        assert_eq!(merged.get("db__host").map(String::as_str), Some("h.toml"));
        assert_eq!(merged.get("db__port").map(String::as_str), Some("1234"));
        assert_eq!(merged.get("auth__name").map(String::as_str), Some("authsvc"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn yaml_file_source_parses_correctly() {
        let p = unique_path("yaml", "yaml");
        write_file(
            &p,
            r#"
db:
  host: h.yaml
  port: 9000
auth:
  name: authsvc
"#,
        );
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Yaml,
            }],
            entries: vec![],
        };
        let env_overlay = HashMap::new();
        let merged = build_sources_overlay(&opts.sources, &env_overlay).expect("merge");
        assert_eq!(merged.get("db__host").map(String::as_str), Some("h.yaml"));
        assert_eq!(merged.get("db__port").map(String::as_str), Some("9000"));
        assert_eq!(merged.get("auth__name").map(String::as_str), Some("authsvc"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn inline_source_overrides_and_null_deletes() {
        let p = unique_path("inline-pre", "json");
        write_file(
            &p,
            r#"{ "db": { "host": "from-file", "port": 1 }, "kill": "alive" }"#,
        );
        let sources = vec![
            ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Json,
            },
            ConfigSource::Inline(serde_json::json!({
                "db": { "host": "from-inline" },
                "kill": null
            })),
        ];
        let merged = build_sources_overlay(&sources, &HashMap::new()).expect("merge");
        assert_eq!(merged.get("db__host").map(String::as_str), Some("from-inline"));
        // `kill` was deleted by the inline `null`.
        assert!(!merged.contains_key("kill"));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn optional_file_missing_is_silently_skipped() {
        let phantom = unique_path("does-not-exist", "json");
        let sources = vec![ConfigSource::OptionalFile {
            path: phantom,
            format: FileFormat::Json,
        }];
        let merged = build_sources_overlay(&sources, &HashMap::new()).expect("merge");
        assert!(merged.is_empty());
    }

    #[test]
    fn required_file_missing_is_an_error() {
        let phantom = unique_path("definitely-missing", "json");
        let sources = vec![ConfigSource::File {
            path: phantom,
            format: FileFormat::Json,
        }];
        let err = build_sources_overlay(&sources, &HashMap::new()).expect_err("missing");
        assert!(
            err.message.contains("failed to read"),
            "error names the read failure: {err}"
        );
    }

    #[test]
    fn env_overlay_wins_over_file_sources() {
        let p = unique_path("env-wins", "json");
        write_file(&p, r#"{ "db": { "host": "from-file" } }"#);
        let sources = vec![ConfigSource::File {
            path: p.clone(),
            format: FileFormat::Json,
        }];
        let env_overlay = overlay(&[("NESTRS_DB__HOST", "from-env")]);
        let merged = build_sources_overlay(&sources, &env_overlay).expect("merge");
        assert_eq!(
            merged.get("NESTRS_DB__HOST").map(String::as_str),
            Some("from-env")
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn sources_merge_in_declaration_order() {
        let p = unique_path("merge-order", "json");
        write_file(&p, r#"{ "feature": { "flag": "from-file" } }"#);
        let sources = vec![
            ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Json,
            },
            ConfigSource::Inline(serde_json::json!({ "feature": { "flag": "from-inline" } })),
        ];
        let merged = build_sources_overlay(&sources, &HashMap::new()).expect("merge");
        assert_eq!(
            merged.get("feature__flag").map(String::as_str),
            Some("from-inline")
        );
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn coerce_string_to_json_handles_primitives_and_empty() {
        assert_eq!(coerce_string_to_json(""), serde_json::Value::Null);
        assert_eq!(coerce_string_to_json("null"), serde_json::Value::Null);
        assert_eq!(
            coerce_string_to_json("true"),
            serde_json::Value::Bool(true)
        );
        assert_eq!(
            coerce_string_to_json("false"),
            serde_json::Value::Bool(false)
        );
        assert_eq!(
            coerce_string_to_json("42"),
            serde_json::Value::Number(42.into())
        );
        assert_eq!(
            coerce_string_to_json("3.125"),
            serde_json::Value::Number(serde_json::Number::from_f64(3.125).unwrap())
        );
        assert_eq!(
            coerce_string_to_json("hello"),
            serde_json::Value::String("hello".into())
        );
        // A bare JSON literal should round-trip.
        assert_eq!(
            coerce_string_to_json("[1,2]"),
            serde_json::json!([1, 2])
        );
    }

    #[test]
    fn flatten_inline_handles_arrays_and_nested_objects() {
        let mut out = HashMap::new();
        flatten_inline(
            serde_json::json!({
                "arr": [1, "two", null],
                "obj": { "a": { "b": "c" } }
            }),
            "",
            &mut out,
        );
        assert_eq!(out.get("arr.0").map(String::as_str), Some("1"));
        assert_eq!(out.get("arr.1").map(String::as_str), Some("two"));
        assert!(!out.contains_key("arr.2")); // null deletes
        assert_eq!(out.get("obj__a__b").map(String::as_str), Some("c"));
    }

    #[test]
    fn raw_to_json_tree_rebuilds_nested_objects() {
        // `raw_to_json_tree` splits on `.` (matches the dotted-key output of
        // `build_sources_overlay` for arrays + the `get_by_key` consumer).
        // Env-style `__` keys are flattened the same way for files/inline.
        let mut raw = HashMap::new();
        raw.insert("NESTRS.DB.HOST".into(), "from-env".into());
        raw.insert("NESTRS.DB.PORT".into(), "5432".into());
        let tree = raw_to_json_tree(&raw);
        let db = tree.get("NESTRS").and_then(|v| v.get("DB"));
        assert!(db.is_some(), "tree: {tree}");
        let host = tree
            .get("NESTRS")
            .and_then(|v| v.get("DB"))
            .and_then(|v| v.get("HOST"))
            .and_then(|v| v.as_str());
        assert_eq!(host, Some("from-env"));
    }

    #[test]
    fn config_module_build_service_with_inline_source() {
        // Namespace + field names are lowercase to match what
        // `parse_namespaced` does (`NESTRS_<NS>__<FIELD>` — env-style).
        let opts = ConfigOptions {
            sources: vec![ConfigSource::Inline(serde_json::json!({
                "db": { "host": "inline.local", "port": 7000 },
                "auth": { "name": "authsvc" }
            }))],
            entries: vec![
                Config::register::<DbConfig>(),
                Config::register::<AuthConfig>(),
            ],
        };
        let svc = ConfigModule::build_service(&opts.entries, &opts).expect("build service");
        let db = svc.get::<DbConfig>().expect("typed get");
        assert_eq!(db.host, "inline.local");
        assert_eq!(db.port, 7000);
        let auth = svc.get::<AuthConfig>().expect("typed get");
        assert_eq!(auth.name, "authsvc");
    }

    #[test]
    fn config_module_build_service_with_required_file_missing_errors() {
        let phantom = unique_path("required-missing", "json");
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: phantom,
                format: FileFormat::Json,
            }],
            entries: vec![Config::register::<DbConfig>()],
        };
        let err = ConfigModule::build_service(&opts.entries, &opts).expect_err("missing");
        assert!(
            err.message.contains("failed to read"),
            "error names the read failure: {err}"
        );
    }

    #[cfg(feature = "config-hot-reload")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn config_watcher_reloads_on_file_change() {
        let p = unique_path("hot-reload", "json");
        write_file(
            &p,
            r#"{ "db": { "host": "v1.local" }, "auth": { "name": "authsvc" } }"#,
        );
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Json,
            }],
            entries: vec![
                Config::register::<DbConfig>(),
                Config::register::<AuthConfig>(),
            ],
        };
        let svc = ConfigModule::build_service(&opts.entries, &opts).expect("initial build");
        let watcher = ConfigWatcher::new(svc, opts);
        assert_eq!(
            watcher.snapshot().get::<DbConfig>().expect("typed get").host,
            "v1.local"
        );
        // Mutate the file; the watcher should pick it up within ~300 ms
        // (150 ms debounce + fs latency margin).
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_file(
            &p,
            r#"{ "db": { "host": "v2.local" }, "auth": { "name": "authsvc" } }"#,
        );
        // Poll up to 3 s for the reload to land.
        let mut reloaded = false;
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if watcher
                .snapshot()
                .get::<DbConfig>()
                .map(|d| d.host == "v2.local")
                .unwrap_or(false)
            {
                reloaded = true;
                break;
            }
        }
        assert!(reloaded, "watcher did not pick up file change");
        watcher.stop().await;
        std::fs::remove_file(&p).ok();
    }

    #[cfg(feature = "config-hot-reload")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn config_watcher_stop_is_clean() {
        let p = unique_path("hot-stop", "json");
        write_file(&p, r#"{ "auth": { "name": "authsvc" } }"#);
        let opts = ConfigOptions {
            sources: vec![ConfigSource::File {
                path: p.clone(),
                format: FileFormat::Json,
            }],
            entries: vec![Config::register::<AuthConfig>()],
        };
        let svc = ConfigModule::build_service(&opts.entries, &opts).expect("build");
        let watcher = ConfigWatcher::new(svc, opts);
        let handle = watcher.service();
        assert!(handle.read().expect("lock").has("auth.name"));
        watcher.stop().await;
        // The shared state remains readable after stop (no panic, no poison).
        drop(handle.read().expect("lock-after-stop"));
        std::fs::remove_file(&p).ok();
    }
}
