use std::sync::Arc;
use std::sync::OnceLock;

use nestrs::prelude::*;

/// Recommended default location for a Prisma schema in nestrs apps.
pub const DEFAULT_SCHEMA_PATH: &str = "prisma/schema.prisma";

/// Builds the documented Rust Prisma client generation command.
///
/// This mirrors the expected `prisma-client-rust-cli` workflow while keeping
/// command construction explicit for docs/tools.
pub fn prisma_generate_command(schema_path: &str) -> String {
    format!("cargo prisma generate --schema {}", schema_path)
}

#[derive(Debug, Clone)]
pub struct PrismaOptions {
    pub database_url: String,
    pub pool_min: u32,
    pub pool_max: u32,
    pub schema_path: String,
}

impl PrismaOptions {
    pub fn from_url(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            pool_min: 2,
            pool_max: 20,
            schema_path: DEFAULT_SCHEMA_PATH.to_string(),
        }
    }

    pub fn pool_min(mut self, value: u32) -> Self {
        self.pool_min = value;
        self
    }

    pub fn pool_max(mut self, value: u32) -> Self {
        self.pool_max = value;
        self
    }

    pub fn schema_path(mut self, value: impl Into<String>) -> Self {
        self.schema_path = value.into();
        self
    }
}

static PRISMA_OPTIONS: OnceLock<PrismaOptions> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct PrismaClientHandle {
    pub database_url: String,
    pub schema_path: String,
}

/// Injectable Prisma service abstraction.
///
/// Current implementation is a stable scaffold over configuration and a simple client
/// handle. A future iteration can replace `PrismaClientHandle` with a concrete
/// generated Prisma Rust client while preserving this public API shape.
pub struct PrismaService {
    options: PrismaOptions,
    client: PrismaClientHandle,
}

impl PrismaService {
    pub fn client(&self) -> &PrismaClientHandle {
        &self.client
    }

    pub fn options(&self) -> &PrismaOptions {
        &self.options
    }

    pub fn health(&self) -> &'static str {
        "ok"
    }

    pub fn query_raw(&self, sql: &str) -> String {
        format!("query accepted by prisma stub: {}", sql)
    }

    /// DTO ↔ model mapping guidance helper for docs/diagnostics.
    ///
    /// This intentionally keeps examples close to NestJS service patterns:
    /// map generated model rows into response DTOs at the service boundary.
    pub fn mapping_guidance(&self) -> &'static str {
        "Prefer `From<ModelData>` / `TryFrom<ModelData>` impls for response DTOs; avoid returning generated Prisma model types directly from controllers."
    }
}

impl Default for PrismaService {
    fn default() -> Self {
        let options = PRISMA_OPTIONS
            .get()
            .cloned()
            .or_else(|| {
                std::env::var("DATABASE_URL")
                    .ok()
                    .map(PrismaOptions::from_url)
            })
            .unwrap_or_else(|| PrismaOptions::from_url("file:./dev.db"));

        let client = PrismaClientHandle {
            database_url: options.database_url.clone(),
            schema_path: options.schema_path.clone(),
        };

        Self { options, client }
    }
}

impl Injectable for PrismaService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[module(
    providers = [PrismaService],
    exports = [PrismaService],
)]
pub struct PrismaModule;

impl PrismaModule {
    /// Nest-like global module configuration entrypoint.
    ///
    /// Call this before `NestFactory::create::<AppModule>()`.
    pub fn for_root(database_url: impl Into<String>) -> Self {
        let _ = PRISMA_OPTIONS.set(PrismaOptions::from_url(database_url));
        Self
    }

    /// More explicit options-based variant.
    pub fn for_root_with_options(options: PrismaOptions) -> Self {
        let _ = PRISMA_OPTIONS.set(options);
        Self
    }

    /// Returns a sample generation command for the currently configured schema.
    /// Useful for startup logs and docs output.
    pub fn generate_command_hint() -> String {
        let schema_path = PRISMA_OPTIONS
            .get()
            .map(|o| o.schema_path.as_str())
            .unwrap_or(DEFAULT_SCHEMA_PATH);
        prisma_generate_command(schema_path)
    }
}

