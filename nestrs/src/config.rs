use crate::core::{Injectable, Module, ProviderRegistry};
use axum::Router;
use serde::de::DeserializeOwned;
use std::any::TypeId;
use std::marker::PhantomData;
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

/// Nest-like module for typed config providers.
///
/// Usage:
/// - Define your config struct: `#[derive(serde::Deserialize, validator::Validate, nestrs::NestConfig)] struct AppConfig { ... }`
/// - Import it: `#[module(imports = [nestrs::ConfigModule::<AppConfig>], ...)]`
pub struct ConfigModule<T>(PhantomData<T>);

impl<T> Default for ConfigModule<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T> Module for ConfigModule<T>
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

impl<T> crate::core::ModuleGraph for ConfigModule<T>
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

