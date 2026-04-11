extern crate self as nestrs;

pub use async_trait::async_trait;
#[doc(hidden)]
pub use axum;
#[doc(hidden)]
pub use serde_json;
use axum::body::{to_bytes, Body};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
pub use nestrs_macros::{
    all, controller, cron, delete, dto, event_pattern, get, head, http_code, injectable,
    interval, message_pattern, micro_routes, module, on_event, event_routes, options, patch, post,
    put, redirect, schedule_routes, queue_processor,
    response_header, roles, routes, set_metadata, raw_body, sse, subscribe_message, use_filters,
    use_guards,
    use_interceptors, use_pipes, serialize, ver, version, ws_gateway, ws_routes, NestConfig, NestDto,
};
use std::sync::OnceLock;
use validator::Validate;

static PROMETHEUS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
static TRACING_SUBSCRIBER: OnceLock<Result<(), String>> = OnceLock::new();
#[cfg(feature = "otel")]
static OTEL_INSTALLED: OnceLock<()> = OnceLock::new();

fn init_prometheus_recorder() -> &'static PrometheusHandle {
    PROMETHEUS_HANDLE.get_or_init(|| {
        let handle = PrometheusBuilder::new()
            .set_buckets_for_metric(
                Matcher::Full("http_request_duration_seconds".to_owned()),
                &[
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ],
            )
            .expect("nestrs: invalid Prometheus histogram buckets")
            .install_recorder()
            .expect("nestrs: failed to install Prometheus metrics recorder");
        let upkeep = handle.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            upkeep.run_upkeep();
        });
        handle.clone()
    })
}

pub mod core {
    pub use nestrs_core::*;
}

#[cfg(feature = "graphql")]
pub use nestrs_graphql as graphql;
#[cfg(feature = "microservices")]
pub use nestrs_microservices as microservices;
#[cfg(feature = "microservices")]
pub use nestrs_events::EventBus;
#[cfg(feature = "microservices")]
mod microservice_health;
#[cfg(feature = "microservices")]
pub use microservice_health::BrokerHealthStub;
#[cfg(all(feature = "microservices", feature = "microservices-nats"))]
pub use microservice_health::NatsBrokerHealth;
#[cfg(all(feature = "microservices", feature = "microservices-redis"))]
pub use microservice_health::RedisBrokerHealth;
#[cfg(feature = "openapi")]
pub use nestrs_openapi as openapi;
#[cfg(feature = "ws")]
pub use nestrs_ws as ws;

mod config;
mod cache;
mod i18n;
#[cfg(feature = "otel")]
pub mod otel;
#[cfg(feature = "schedule")]
pub mod schedule;
#[cfg(feature = "queues")]
pub mod queues;
mod client_ip;
mod exception_filter;
mod interceptor;
mod raw_body;
mod pipes;
mod request_context;
mod request_scoped;
pub mod sse;
pub mod multipart;
mod testing;
mod versioning;
pub mod problem;

pub use cache::{CacheError, CacheModule, CacheOptions, CacheService};
#[cfg(feature = "cache-redis")]
pub use cache::RedisCacheOptions;
pub use i18n::{I18n, I18nMissing, I18nModule, I18nOptions, I18nService, Locale};
#[cfg(feature = "otel")]
pub use otel::{OpenTelemetryConfig, OtlpProtocol};
#[cfg(feature = "schedule")]
pub use schedule::{ScheduleModule, ScheduleRuntime};
#[cfg(feature = "queues")]
pub use queues::{
    JobOptions, QueueConfig, QueueError, QueueHandler, QueueHandle, QueueJob, QueuesModule,
    QueuesService, QueuesRuntime,
};
pub use client_ip::{ClientIp, ClientIpMissing};
pub use config::{load_config, ConfigError, ConfigModule};
pub use exception_filter::ExceptionFilter;
pub use interceptor::{Interceptor, LoggingInterceptor};
pub use pipes::ParseIntPipe;
pub use pipes::ValidationPipe;
pub use raw_body::RawBody;
pub use request_context::{RequestContext, RequestContextMissing};
pub use request_scoped::{RequestScoped, RequestScopedMissing};
pub use testing::{TestClient, TestRequest, TestingModule, TestingModuleBuilder};
pub use problem::ProblemDetails;
pub use versioning::{host_restriction_middleware, ApiVersioningPolicy, NestApiVersion, VersioningType};

/// Axum middleware from an [`Interceptor`](Interceptor) type (uses `I::default()` per request).
#[macro_export]
macro_rules! interceptor_layer {
    ($I:ty) => {
        ::axum::middleware::from_fn(
            |req: ::axum::extract::Request, next: ::axum::middleware::Next| async move {
                let i: $I = ::core::default::Default::default();
                $crate::Interceptor::intercept(&i, req, next).await
            },
        )
    };
}

pub mod prelude {
    pub use crate::core::{
        AuthError, AuthStrategy, CanActivate, ConfigurableModuleBuilder, Controller, DynamicModule,
        DynamicModuleBuilder, GuardError, Injectable, MetadataRegistry, Module, ModuleOptions,
        PipeTransform, ProviderRegistry, ProviderScope,
    };
    #[cfg(feature = "graphql")]
    pub use crate::graphql;
    pub use crate::interceptor_layer;
    #[cfg(feature = "microservices")]
    pub use crate::microservices;
    #[cfg(feature = "microservices")]
    pub use crate::microservices::{
        ClientConfig, ClientProxy, ClientsModule, ClientsService, EventBus, KafkaTransport, MqttTransport,
        Transport, TransportError,
    };
    #[cfg(feature = "microservices")]
    pub use crate::BrokerHealthStub;
    #[cfg(all(feature = "microservices", feature = "microservices-nats"))]
    pub use crate::NatsBrokerHealth;
    #[cfg(all(feature = "microservices", feature = "microservices-redis"))]
    pub use crate::RedisBrokerHealth;
    #[cfg(all(feature = "microservices", feature = "microservices-kafka"))]
    pub use crate::microservices::{
        kafka_cluster_reachable_with, KafkaConnectionOptions, KafkaMicroserviceOptions,
        KafkaMicroserviceServer, KafkaSaslOptions, KafkaTlsOptions, KafkaTransportOptions,
    };
    #[cfg(all(feature = "microservices", feature = "microservices-mqtt"))]
    pub use crate::microservices::{
        MqttMicroserviceOptions, MqttMicroserviceServer, MqttSocketOptions, MqttTlsMode,
        MqttTransportOptions,
    };
    #[cfg(feature = "queues")]
    pub use crate::queues;
    #[cfg(feature = "otel")]
    pub use crate::otel;
    #[cfg(feature = "queues")]
    pub use crate::{
        JobOptions, QueueConfig, QueueError, QueueHandler, QueueHandle, QueueJob, QueuesModule,
        QueuesRuntime, QueuesService,
    };
    #[cfg(feature = "otel")]
    pub use crate::{OpenTelemetryConfig, OtlpProtocol};
    #[cfg(feature = "schedule")]
    pub use crate::{ScheduleModule, ScheduleRuntime};
    #[cfg(feature = "openapi")]
    pub use crate::openapi;
    #[cfg(feature = "ws")]
    pub use crate::ws;
    pub use crate::{
        all, async_trait, controller, cron, delete, dto, event_pattern, get, head, http_code,
        impl_routes, injectable, message_pattern, module, nestrs_default_not_found_handler,
        interval, on_event, event_routes, options, patch, post, put, redirect, response_header, roles, routes,
        schedule_routes, queue_processor,
        runtime_is_production, set_metadata, try_init_tracing, use_filters, use_guards,
        micro_routes, raw_body, serialize, sse, subscribe_message, use_interceptors, use_pipes,
        ver, version, ws_gateway, ws_routes, BadGatewayException, BadRequestException,
        CacheError, CacheModule, CacheOptions, CacheService,
        I18n, I18nMissing, I18nModule, I18nOptions, I18nService, Locale,
        ClientIp, ClientIpMissing, ConflictException, CorsOptions, ExceptionFilter, ForbiddenException,
        GatewayTimeoutException, GoneException, HealthIndicator, HealthStatus, HttpException,
        Interceptor, InternalServerErrorException, LoggingInterceptor, MethodNotAllowedException,
        load_config, ConfigError, ConfigModule, ApiVersioningPolicy, NestApiVersion, NestApplication,
        NestConfig, NestDto, NestFactory, VersioningType,
        NotAcceptableException, NotFoundException, NotImplementedException, ParseIntPipe,
        TestClient, TestRequest, TestingModule, TestingModuleBuilder,
        RawBody,
        ValidationPipe,
        PathNormalization, PayloadTooLargeException,
        PaymentRequiredException, ProblemDetails, RateLimitOptions, ReadinessContext, RequestContext,
        RequestContextMissing, RequestScoped, RequestScopedMissing, RequestTimeoutException,
        RequestTracingOptions, SecurityHeaders, ServiceUnavailableException,
        TooManyRequestsException, TracingConfig, TracingFormat, UnauthorizedException,
        UnprocessableEntityException, UnsupportedMediaTypeException, ValidatedBody, ValidatedPath,
        ValidatedQuery,
    };
    #[cfg(feature = "otel")]
    pub use crate::try_init_tracing_opentelemetry;
    pub use axum::{extract::Multipart, extract::State, response::IntoResponse, Json};
}

/// Returns `true` when the process environment indicates a **production** deployment.
///
/// The first non-empty value among `NESTRS_ENV`, `APP_ENV`, and `RUST_ENV` wins (in that order).
/// Values are compared case-insensitively after trimming. `production` and `prod` are treated as
/// production; any other explicit value (for example `development`, `test`, `staging`) is not.
/// If none of these variables are set, returns `false` (safe default for local development).
pub fn runtime_is_production() -> bool {
    const KEYS: [&str; 3] = ["NESTRS_ENV", "APP_ENV", "RUST_ENV"];
    for key in KEYS {
        if let Ok(raw) = std::env::var(key) {
            let s = raw.trim();
            if s.is_empty() {
                continue;
            }
            let lower = s.to_ascii_lowercase();
            return matches!(lower.as_str(), "production" | "prod");
        }
    }
    false
}

/// Log line format for [`TracingConfig`] / [`try_init_tracing`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TracingFormat {
    /// Human-readable multi-line output (development).
    #[default]
    Pretty,
    /// One JSON object per line (typical for production log aggregation).
    Json,
}

/// Global `tracing` subscriber configuration (env filter + format).
///
/// Directive resolution: **`NESTRS_LOG`** if set, else **`RUST_LOG`**, else [`TracingConfig::default_directive`].
#[derive(Clone, Debug)]
pub struct TracingConfig {
    format: TracingFormat,
    /// Used when neither `NESTRS_LOG` nor `RUST_LOG` is set (same semantics as `RUST_LOG` values).
    default_directive: String,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            format: TracingFormat::Pretty,
            default_directive: "info".to_string(),
        }
    }
}

impl TracingConfig {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn format(mut self, format: TracingFormat) -> Self {
        self.format = format;
        self
    }

    /// Default filter when `NESTRS_LOG` / `RUST_LOG` are unset (e.g. `"info"`, `"nestrs=debug,info"`).
    pub fn default_directive(mut self, directive: impl Into<String>) -> Self {
        self.default_directive = directive.into();
        self
    }
}

fn tracing_env_filter(config: &TracingConfig) -> tracing_subscriber::EnvFilter {
    let raw = std::env::var("NESTRS_LOG")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("RUST_LOG")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
    if let Some(s) = raw {
        tracing_subscriber::EnvFilter::try_new(&s)
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.default_directive))
    } else {
        tracing_subscriber::EnvFilter::new(&config.default_directive)
    }
}

fn install_tracing_subscriber(config: TracingConfig) -> Result<(), String> {
    use tracing_subscriber::prelude::*;

    let filter = tracing_env_filter(&config);
    let registry = tracing_subscriber::registry().with(filter);

    let result = match config.format {
        TracingFormat::Pretty => registry
            .with(tracing_subscriber::fmt::layer().pretty())
            .try_init(),
        TracingFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .try_init(),
    };

    result.map_err(|e| e.to_string())
}

#[cfg(feature = "otel")]
fn install_tracing_subscriber_otel(
    config: TracingConfig,
    otel: OpenTelemetryConfig,
) -> Result<(), String> {
    use tracing_subscriber::prelude::*;

    let filter = tracing_env_filter(&config);
    let tracer = crate::otel::install_otlp_tracer(otel)?;
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let registry = tracing_subscriber::registry().with(filter).with(otel_layer);
    let result = match config.format {
        TracingFormat::Pretty => registry
            .with(tracing_subscriber::fmt::layer().pretty())
            .try_init(),
        TracingFormat::Json => registry
            .with(tracing_subscriber::fmt::layer().json())
            .try_init(),
    };

    result.map_err(|e| e.to_string())
}

/// Installs the global `tracing` subscriber once (idempotent). Subsequent calls return the same result as the first.
///
/// If a global subscriber was already installed (e.g. by tests or another crate), installation errors that indicate
/// "already initialized" are treated as success so `configure_tracing` can be used in mixed environments.
pub fn try_init_tracing(config: TracingConfig) -> Result<(), String> {
    TRACING_SUBSCRIBER
        .get_or_init(|| match install_tracing_subscriber(config) {
            Ok(()) => Ok(()),
            Err(msg) if msg.to_lowercase().contains("already") => Ok(()),
            Err(e) => Err(e),
        })
        .clone()
}

/// Installs a global `tracing` subscriber that exports spans to an OTLP collector (OpenTelemetry).
///
/// Feature-gated behind `otel`. The first successful tracing initialization wins; subsequent calls
/// return the result of the first initializer.
#[cfg(feature = "otel")]
pub fn try_init_tracing_opentelemetry(
    config: TracingConfig,
    otel: OpenTelemetryConfig,
) -> Result<(), String> {
    TRACING_SUBSCRIBER
        .get_or_init(|| match install_tracing_subscriber_otel(config, otel) {
            Ok(()) => {
                let _ = OTEL_INSTALLED.set(());
                Ok(())
            }
            Err(msg) if msg.to_lowercase().contains("already") => Ok(()),
            Err(e) => Err(e),
        })
        .clone()
}
pub struct NestFactory;
pub trait NestDto {}
pub trait NestConfig {}

/// Result of a single [`HealthIndicator::check`].
#[derive(Debug, Clone)]
pub enum HealthStatus {
    Up,
    Down { message: String },
}

impl HealthStatus {
    pub fn down(message: impl Into<String>) -> Self {
        Self::Down {
            message: message.into(),
        }
    }
}

/// Pluggable readiness check (database ping, broker, external HTTP, etc.).
#[async_trait]
pub trait HealthIndicator: Send + Sync {
    fn name(&self) -> &'static str;

    async fn check(&self) -> HealthStatus;
}

/// Holds indicators for [`NestApplication::enable_readiness_check`]; exposed so apps can reuse or test checks.
#[derive(Clone)]
pub struct ReadinessContext {
    indicators: Vec<std::sync::Arc<dyn HealthIndicator>>,
}

impl ReadinessContext {
    pub fn indicators(&self) -> &[std::sync::Arc<dyn HealthIndicator>] {
        &self.indicators
    }
}

#[derive(Clone, Debug, Default)]
pub struct RequestTracingOptions {
    skip_paths: Vec<String>,
}

impl RequestTracingOptions {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn skip_paths<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.skip_paths = paths
            .into_iter()
            .map(|p| {
                let s = p.as_ref().trim();
                if s.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", s.trim_matches('/'))
                }
            })
            .collect();
        self
    }
}

#[derive(Clone, Debug)]
pub struct RateLimitOptions {
    max_requests: u64,
    window_secs: u64,
    #[cfg(feature = "cache-redis")]
    redis: Option<RedisRateLimitOptions>,
}

/// Redis-backed fixed window counter (shared across instances). Requires **`cache-redis`**.
#[cfg(feature = "cache-redis")]
#[derive(Clone, Debug)]
pub struct RedisRateLimitOptions {
    pub url: String,
    pub key_prefix: String,
}

impl Default for RateLimitOptions {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_secs: 60,
            #[cfg(feature = "cache-redis")]
            redis: None,
        }
    }
}

impl RateLimitOptions {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn max_requests(mut self, value: u64) -> Self {
        self.max_requests = value.max(1);
        self
    }

    pub fn window_secs(mut self, value: u64) -> Self {
        self.window_secs = value.max(1);
        self
    }

    /// Distributed rate limiting via Redis `INCR` + `EXPIRE` (per client IP).
    #[cfg(feature = "cache-redis")]
    pub fn redis(mut self, url: impl Into<String>, key_prefix: impl Into<String>) -> Self {
        self.redis = Some(RedisRateLimitOptions {
            url: url.into(),
            key_prefix: key_prefix.into(),
        });
        self
    }

    pub fn build(self) -> Self {
        self
    }
}

#[derive(Clone, Debug)]
pub struct SecurityHeaders {
    x_content_type_options: Option<String>,
    x_frame_options: Option<String>,
    referrer_policy: Option<String>,
    x_xss_protection: Option<String>,
    permissions_policy: Option<String>,
    content_security_policy: Option<String>,
    hsts: Option<String>,
}

impl Default for SecurityHeaders {
    fn default() -> Self {
        Self {
            x_content_type_options: Some("nosniff".to_string()),
            x_frame_options: Some("DENY".to_string()),
            referrer_policy: Some("strict-origin-when-cross-origin".to_string()),
            x_xss_protection: Some("0".to_string()),
            permissions_policy: Some("geolocation=(), microphone=(), camera=()".to_string()),
            content_security_policy: None,
            hsts: None,
        }
    }
}

impl SecurityHeaders {
    pub fn content_security_policy(mut self, value: impl Into<String>) -> Self {
        self.content_security_policy = Some(value.into());
        self
    }

    pub fn hsts(mut self, value: impl Into<String>) -> Self {
        self.hsts = Some(value.into());
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct CorsOptions {
    permissive: bool,
    allow_origins: Vec<String>,
    allow_methods: Vec<axum::http::Method>,
    allow_headers: Vec<axum::http::header::HeaderName>,
    allow_credentials: bool,
    max_age_secs: Option<u64>,
}

impl CorsOptions {
    pub fn permissive() -> Self {
        Self {
            permissive: true,
            ..Self::default()
        }
    }

    pub fn builder() -> Self {
        Self::default()
    }

    pub fn allow_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allow_origins = origins.into_iter().map(Into::into).collect();
        self
    }

    pub fn allow_methods<I, S>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.allow_methods = methods
            .into_iter()
            .filter_map(|m| m.as_ref().parse::<axum::http::Method>().ok())
            .collect();
        self
    }

    pub fn allow_headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.allow_headers = headers
            .into_iter()
            .filter_map(|h| h.as_ref().parse::<axum::http::header::HeaderName>().ok())
            .collect();
        self
    }

    pub fn allow_credentials(mut self, value: bool) -> Self {
        self.allow_credentials = value;
        self
    }

    pub fn max_age_secs(mut self, secs: u64) -> Self {
        self.max_age_secs = Some(secs);
        self
    }

    pub fn build(self) -> Self {
        self
    }

    fn is_permissive(&self) -> bool {
        self.permissive
    }

    fn into_layer(self) -> tower_http::cors::CorsLayer {
        if self.permissive {
            return tower_http::cors::CorsLayer::permissive();
        }

        let mut layer = tower_http::cors::CorsLayer::new();
        if !self.allow_origins.is_empty() {
            if self.allow_origins.iter().any(|o| o == "*") {
                layer = layer.allow_origin(tower_http::cors::Any);
            } else {
                let origins = self
                    .allow_origins
                    .iter()
                    .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
                    .collect::<Vec<_>>();
                if !origins.is_empty() {
                    layer = layer.allow_origin(origins);
                }
            }
        }
        if !self.allow_methods.is_empty() {
            layer = layer.allow_methods(self.allow_methods);
        }
        if !self.allow_headers.is_empty() {
            layer = layer.allow_headers(self.allow_headers);
        }
        if self.allow_credentials {
            layer = layer.allow_credentials(true);
        }
        if let Some(secs) = self.max_age_secs {
            layer = layer.max_age(std::time::Duration::from_secs(secs));
        }
        layer
    }
}

/// How [`NestApplication::use_path_normalization`] rewrites the request URI before routing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathNormalization {
    /// `/items/` → `/items` (common for REST APIs).
    TrimTrailingSlash,
    /// `/items` → `/items/`.
    AppendTrailingSlash,
}

pub struct NestApplication {
    registry: std::sync::Arc<crate::core::ProviderRegistry>,
    router: axum::Router,
    uri_version: Option<String>,
    /// Header or `Accept`-based versioning (URI versioning uses [`Self::enable_uri_versioning`]).
    api_versioning: Option<ApiVersioningPolicy>,
    global_prefix: Option<String>,
    /// Static file mounts at the server root (not under URI versioning / global prefix).
    static_mounts: Vec<(String, std::path::PathBuf)>,
    cors_options: Option<CorsOptions>,
    security_headers: Option<SecurityHeaders>,
    rate_limit_options: Option<RateLimitOptions>,
    request_timeout: Option<std::time::Duration>,
    /// Max in-flight requests for the full app service (Tower `ConcurrencyLimitLayer`).
    concurrency_limit: Option<usize>,
    /// When enabled, sheds excess load immediately when inner services are not ready.
    load_shed: bool,
    body_limit_bytes: Option<usize>,
    production_errors: bool,
    request_id: bool,
    /// Injects [`RequestContext`] into each request’s extensions (see [`Self::use_request_context`]).
    request_context: bool,
    /// Enables request-scoped providers (`ProviderScope::Request`) via task-local cache + per-request registry injection.
    request_scope: bool,
    /// Resolves locale per request and installs [`crate::Locale`] / [`crate::I18n`] extractors (see [`Self::use_i18n`]).
    i18n: bool,
    /// GET route for liveness (merged at server root, not under [`Self::enable_uri_versioning`] / [`Self::set_global_prefix`]).
    liveness_path: Option<String>,
    /// GET readiness route at server root + indicator list (see [`Self::enable_readiness_check`]).
    readiness: Option<(String, Vec<std::sync::Arc<dyn HealthIndicator>>)>,
    /// Prometheus scrape path at server root (see [`Self::enable_metrics`]).
    metrics_path: Option<String>,
    #[cfg(feature = "openapi")]
    openapi: Option<nestrs_openapi::OpenApiOptions>,
    request_tracing: Option<RequestTracingOptions>,
    /// User-defined Tower layers applied **outermost** after all built-in middleware (see [`Self::use_global_layer`]).
    global_layers: Vec<GlobalLayerFn>,
    /// Optional global [`ExceptionFilter`] (runs just above route services, before CORS and production sanitization).
    exception_filter: Option<std::sync::Arc<dyn ExceptionFilter>>,
    /// When true, install [`nestrs_default_not_found_handler`] as the router fallback (Nest-style JSON 404).
    default_404_fallback: bool,
    /// When true, compress eligible responses when the client sends a matching `Accept-Encoding` (gzip).
    compression: bool,
    /// When true, decompress request bodies when `Content-Encoding: gzip` is set (see [`Self::use_request_decompression`]).
    request_decompression: bool,
    /// Address for [`Self::listen`], [`Self::listen_with_shutdown`], [`Self::listen_graceful`]. `None` ⇒ `127.0.0.1`.
    listen_ip: Option<std::net::IpAddr>,
    /// Applied in [`Self::listen`] / [`Self::listen_graceful`] only (Axum 0.7: not via [`axum::Router::layer`]).
    /// Cleared for [`Self::into_router`]; wrap the router yourself if you call [`Self::into_router`] and need normalization.
    path_normalization: Option<PathNormalization>,
}

type GlobalLayerFn = Box<dyn Fn(axum::Router) -> axum::Router + Send + Sync + 'static>;

impl NestFactory {
    pub fn create<M: core::Module>() -> NestApplication {
        let (registry, router) = M::build();
        NestApplication {
            registry: std::sync::Arc::new(registry),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        }
    }

    /// Creates an app from a root static module plus runtime-selected dynamic modules.
    ///
    /// Useful for conditional feature routers (for example `if cfg!(feature = "...")` imports).
    /// Dynamic modules can compose routing trees and export providers into the root module registry.
    pub fn create_with_modules<M, I>(dynamic_modules: I) -> NestApplication
    where
        M: core::Module,
        I: IntoIterator<Item = core::DynamicModule>,
    {
        let (mut registry, mut router) = M::build();
        for dm in dynamic_modules {
            registry.absorb_exported(dm.registry, &dm.exports);
            router = router.merge(dm.router);
        }
        NestApplication {
            registry: std::sync::Arc::new(registry),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        }
    }

    /// Creates a microservice app (feature: `microservices`) using the TCP transport adapter.
    ///
    /// Use `.also_listen_http(port)` to run HTTP + microservice in one process (hybrid bootstrap).
    #[cfg(feature = "microservices")]
    pub fn create_microservice<M>(options: crate::microservices::TcpMicroserviceOptions) -> MicroserviceApplication
    where
        M: core::Module + crate::microservices::MicroserviceModule,
    {
        let (registry, router) = M::build();
        let registry = std::sync::Arc::new(registry);

        let handlers = M::microservice_handlers()
            .into_iter()
            .map(|f| f(&registry))
            .collect::<Vec<_>>();

        let server: Box<dyn crate::microservices::MicroserviceServer> =
            Box::new(crate::microservices::TcpMicroserviceServer::new(options, handlers));

        let http = NestApplication {
            registry: registry.clone(),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        };

        MicroserviceApplication {
            registry,
            http,
            server,
            http_port: None,
        }
    }

    /// Creates a microservice app (feature: `microservices-nats`) using the NATS transport adapter.
    #[cfg(feature = "microservices-nats")]
    pub fn create_microservice_nats<M>(
        options: crate::microservices::NatsMicroserviceOptions,
    ) -> MicroserviceApplication
    where
        M: core::Module + crate::microservices::MicroserviceModule,
    {
        let (registry, router) = M::build();
        let registry = std::sync::Arc::new(registry);

        let handlers = M::microservice_handlers()
            .into_iter()
            .map(|f| f(&registry))
            .collect::<Vec<_>>();

        let server: Box<dyn crate::microservices::MicroserviceServer> =
            Box::new(crate::microservices::NatsMicroserviceServer::new(options, handlers));

        let http = NestApplication {
            registry: registry.clone(),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        };

        MicroserviceApplication {
            registry,
            http,
            server,
            http_port: None,
        }
    }

    /// Creates a microservice app (feature: `microservices-redis`) using the Redis transport adapter.
    #[cfg(feature = "microservices-redis")]
    pub fn create_microservice_redis<M>(
        options: crate::microservices::RedisMicroserviceOptions,
    ) -> MicroserviceApplication
    where
        M: core::Module + crate::microservices::MicroserviceModule,
    {
        let (registry, router) = M::build();
        let registry = std::sync::Arc::new(registry);

        let handlers = M::microservice_handlers()
            .into_iter()
            .map(|f| f(&registry))
            .collect::<Vec<_>>();

        let server: Box<dyn crate::microservices::MicroserviceServer> =
            Box::new(crate::microservices::RedisMicroserviceServer::new(options, handlers));

        let http = NestApplication {
            registry: registry.clone(),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        };

        MicroserviceApplication {
            registry,
            http,
            server,
            http_port: None,
        }
    }

    /// Creates a microservice app (feature: `microservices-grpc`) using the gRPC transport adapter.
    #[cfg(feature = "microservices-grpc")]
    pub fn create_microservice_grpc<M>(
        options: crate::microservices::GrpcMicroserviceOptions,
    ) -> MicroserviceApplication
    where
        M: core::Module + crate::microservices::MicroserviceModule,
    {
        let (registry, router) = M::build();
        let registry = std::sync::Arc::new(registry);

        let handlers = M::microservice_handlers()
            .into_iter()
            .map(|f| f(&registry))
            .collect::<Vec<_>>();

        let server: Box<dyn crate::microservices::MicroserviceServer> =
            Box::new(crate::microservices::GrpcMicroserviceServer::new(options, handlers));

        let http = NestApplication {
            registry: registry.clone(),
            router,
            uri_version: None,
            api_versioning: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        };

        MicroserviceApplication {
            registry,
            http,
            server,
            http_port: None,
        }
    }
}

/// Microservice bootstrap handle (NestJS `createMicroservice` analogue).
#[cfg(feature = "microservices")]
pub struct MicroserviceApplication {
    registry: std::sync::Arc<crate::core::ProviderRegistry>,
    http: NestApplication,
    server: Box<dyn crate::microservices::MicroserviceServer>,
    http_port: Option<u16>,
}

#[cfg(feature = "microservices")]
impl MicroserviceApplication {
    /// Opt-in hybrid mode: run the HTTP router on `port` in the same process.
    pub fn also_listen_http(mut self, port: u16) -> Self {
        self.http_port = Some(port);
        self
    }

    /// Apply additional HTTP configuration before `listen*`.
    pub fn configure_http<F>(mut self, f: F) -> Self
    where
        F: FnOnce(NestApplication) -> NestApplication,
    {
        self.http = f(self.http);
        self
    }

    /// Resolve a provider from the app DI container.
    pub fn get<T>(&self) -> std::sync::Arc<T>
    where
        T: Send + Sync + 'static,
    {
        self.registry.get::<T>()
    }

    pub async fn listen(self) {
        self.listen_with_shutdown(nestrs_shutdown_signal()).await;
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        use tokio::sync::watch;

        let registry = self.registry.clone();
        let mut http = self.http;
        let server = self.server;
        let http_port = self.http_port;

        // Lifecycle hooks are shared across HTTP + microservice (run once).
        registry.eager_init_singletons();
        crate::microservices::wire_on_event_handlers(registry.as_ref());
        registry.run_on_module_init().await;
        registry.run_on_application_bootstrap().await;
        #[cfg(feature = "schedule")]
        crate::schedule::wire_scheduled_tasks(registry.as_ref()).await;
        #[cfg(feature = "queues")]
        crate::queues::wire_queue_processors(registry.as_ref()).await;

        let (tx, rx) = watch::channel(false);
        tokio::spawn(async move {
            shutdown.await;
            let _ = tx.send(true);
        });

        let shutdown_for_ms = {
            let mut rx = rx.clone();
            async move {
                while !*rx.borrow() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            }
        };

        let shutdown_for_ms: crate::microservices::ShutdownFuture = Box::pin(shutdown_for_ms);
        let ms_task = tokio::spawn(async move { server.listen_with_shutdown(shutdown_for_ms).await });

        let http_task = if let Some(port) = http_port {
            let mut rx = rx.clone();
            let shutdown_for_http = async move {
                while !*rx.borrow() {
                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            };

            Some(tokio::spawn(async move {
                let ip = http
                    .listen_ip
                    .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
                let path_normalization = http.path_normalization.take();
                let router = http.build_router();

                let listener = tokio::net::TcpListener::bind((ip, port))
                    .await
                    .unwrap_or_else(|e| panic!("failed to bind on {ip}:{port}: {e}"));

                axum_serve(
                    listener,
                    router,
                    path_normalization,
                    Some(Box::pin(shutdown_for_http)),
                )
                .await;
            }))
        } else {
            None
        };

        let _ = ms_task.await;
        if let Some(t) = http_task {
            let _ = t.await;
        }

        registry.run_on_application_shutdown().await;
        registry.run_on_module_destroy().await;
        #[cfg(feature = "otel")]
        if OTEL_INSTALLED.get().is_some() {
            crate::otel::shutdown_tracer_provider();
        }
    }
}

impl NestApplication {
    fn normalize_segment(input: String) -> String {
        let trimmed = input.trim_matches('/');
        if trimmed.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", trimmed)
        }
    }

    fn normalize_static_mount(path: String) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return "/".to_string();
        }
        let inner = trimmed.trim_matches('/');
        if inner.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", inner)
        }
    }

    pub fn set_global_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.global_prefix = Some(Self::normalize_segment(prefix.into()));
        self
    }

    /// Serve static assets from `dir` at `mount_path` (NestJS `ServeStaticModule` / `useStaticAssets` analogue).
    ///
    /// The static service is mounted at the **server root** (not under URI versioning / global prefix),
    /// similar to health and metrics probes.
    pub fn serve_static(
        mut self,
        mount_path: impl Into<String>,
        dir: impl Into<std::path::PathBuf>,
    ) -> Self {
        self.static_mounts
            .push((Self::normalize_static_mount(mount_path.into()), dir.into()));
        self
    }

    /// Sets the bind address for [`Self::listen`] / [`Self::listen_graceful`] (default **`127.0.0.1`**).
    pub fn set_listen_ip(mut self, ip: std::net::IpAddr) -> Self {
        self.listen_ip = Some(ip);
        self
    }

    /// Listen on **`0.0.0.0`** (all IPv4 interfaces) instead of loopback. Typical for containers and LAN access.
    pub fn bind_all_interfaces(mut self) -> Self {
        self.listen_ip = Some(std::net::Ipv4Addr::UNSPECIFIED.into());
        self
    }

    /// Normalizes request paths **before** route matching using [`tower_http::normalize_path`].
    ///
    /// Wired only in [`Self::listen`], [`Self::listen_with_shutdown`], and [`Self::listen_graceful`]
    /// (Axum 0.7 requires wrapping the [`axum::Router`] with [`tower::Layer::layer`], not [`axum::Router::layer`]).
    /// [`Self::into_router`] **ignores** this setting; for a custom server use
    /// `NormalizePathLayer::trim_trailing_slash().layer(router)` (or append) and
    /// [`axum::ServiceExt::into_make_service`] as in the Axum guide.
    pub fn use_path_normalization(mut self, mode: PathNormalization) -> Self {
        self.path_normalization = Some(mode);
        self
    }

    pub fn enable_uri_versioning(mut self, version: impl Into<String>) -> Self {
        self.uri_version = Some(Self::normalize_segment(version.into()));
        self
    }

    /// Nest [`VersioningType`](https://docs.nestjs.com/techniques/versioning) for **header** or **`Accept`**
    /// negotiation. Sets [`NestApiVersion`] on the request for guards/interceptors. URI versioning remains
    /// [`Self::enable_uri_versioning`].
    pub fn enable_api_versioning(mut self, policy: ApiVersioningPolicy) -> Self {
        self.api_versioning = Some(policy);
        self
    }

    /// Shorthand for header-based versioning (`X-API-Version` by default).
    pub fn enable_header_versioning(
        mut self,
        header_name: impl Into<String>,
        default_version: Option<String>,
    ) -> Self {
        self.api_versioning = Some(ApiVersioningPolicy {
            kind: VersioningType::Header,
            header_name: Some(header_name.into()),
            default_version,
        });
        self
    }

    /// Shorthand for `Accept: ...;version=` style versioning.
    pub fn enable_media_type_versioning(mut self, default_version: Option<String>) -> Self {
        self.api_versioning = Some(ApiVersioningPolicy {
            kind: VersioningType::MediaType,
            header_name: None,
            default_version,
        });
        self
    }

    pub fn enable_cors(mut self, options: CorsOptions) -> Self {
        if options.is_permissive() && runtime_is_production() {
            tracing::warn!(
                target: "nestrs",
                "CORS permissive mode enabled in production environment"
            );
        }
        self.cors_options = Some(options);
        self
    }

    pub fn use_security_headers(mut self, headers: SecurityHeaders) -> Self {
        self.security_headers = Some(headers);
        self
    }

    pub fn use_rate_limit(mut self, options: RateLimitOptions) -> Self {
        self.rate_limit_options = Some(options);
        self
    }

    pub fn use_request_timeout(mut self, duration: std::time::Duration) -> Self {
        self.request_timeout = Some(duration);
        self
    }

    /// Limits the number of in-flight requests across the application.
    ///
    /// Additional requests wait until capacity is available unless [`Self::use_load_shed`] is enabled,
    /// in which case overload is rejected immediately with `503 Service Unavailable`.
    pub fn use_concurrency_limit(mut self, max_in_flight: usize) -> Self {
        self.concurrency_limit = Some(max_in_flight.max(1));
        self
    }

    /// Enables Tower load shedding for the app service.
    ///
    /// Pair with [`Self::use_concurrency_limit`] to reject overload quickly instead of queuing.
    pub fn use_load_shed(mut self) -> Self {
        self.load_shed = true;
        self
    }

    pub fn use_body_limit(mut self, max_bytes: usize) -> Self {
        self.body_limit_bytes = Some(max_bytes.max(1));
        self
    }

    /// Enables **gzip** response compression via [`tower_http::compression::CompressionLayer`].
    ///
    /// Compression follows tower-http defaults (for example bodies under **32** bytes are skipped).
    /// The client must advertise support with `Accept-Encoding: gzip`. Applied as a built-in layer
    /// before [`Self::use_global_layer`] callbacks.
    pub fn use_compression(mut self) -> Self {
        self.compression = true;
        self
    }

    /// Enables **gzip** request body decompression via [`tower_http::decompression::RequestDecompressionLayer`].
    ///
    /// When the client sends `Content-Encoding: gzip`, the handler sees the decoded bytes. Unsupported
    /// `Content-Encoding` values yield **415 Unsupported Media Type** by default (see tower-http docs).
    ///
    /// Layer order: registered **inside** [`Self::use_compression`] when both are enabled (response
    /// compression does not interfere with decompressing the request body).
    pub fn use_request_decompression(mut self) -> Self {
        self.request_decompression = true;
        self
    }

    /// When enabled, JSON bodies for **5xx** responses are sanitized: generic `message`, no `errors` payload.
    /// Aligns with production-safe error responses (no internal detail leakage).
    pub fn enable_production_errors(mut self) -> Self {
        self.production_errors = true;
        self
    }

    /// Enables the same behavior as [`Self::enable_production_errors`] when [`runtime_is_production`] is true.
    pub fn enable_production_errors_from_env(mut self) -> Self {
        self.production_errors = runtime_is_production();
        self
    }

    /// Assigns a stable `x-request-id` on each request (UUID when missing) and echoes it on the response.
    pub fn use_request_id(mut self) -> Self {
        self.request_id = true;
        self
    }

    /// Attaches a [`RequestContext`] snapshot to each request (method, path/query, optional `x-request-id`).
    ///
    /// Register this **before** [`Self::use_request_id`] in source order is not required: the middleware is
    /// ordered so request-id layers run first, then the snapshot sees the final header. Pair with
    /// [`Self::use_request_id`] when you want [`RequestContext::request_id`] populated for new requests.
    pub fn use_request_context(mut self) -> Self {
        self.request_context = true;
        self
    }

    /// Resolves locale per request and installs [`Locale`] / [`I18n`] extractors.
    ///
    /// Requires `I18nService` to be registered (typically via `I18nModule` import).
    pub fn use_i18n(mut self) -> Self {
        self.i18n = true;
        self
    }

    /// Enables request-scoped providers (`ProviderScope::Request`) and the [`RequestScoped`] extractor.
    ///
    /// This installs a middleware that:
    /// - injects the app's provider registry into request extensions
    /// - creates a task-local request-scope cache so repeated resolutions within one request share instances
    pub fn use_request_scope(mut self) -> Self {
        self.request_scope = true;
        self
    }

    /// Registers a minimal JSON **liveness** probe at `path` (for example `"/health"`).
    ///
    /// The route is mounted at the **server root**, not under [`Self::enable_uri_versioning`] or
    /// [`Self::set_global_prefix`], so orchestrators can probe `GET /health` without repeating API prefixes.
    pub fn enable_health_check(mut self, path: impl Into<String>) -> Self {
        self.liveness_path = Some(Self::normalize_health_path(path.into()));
        self
    }

    /// **Readiness** probe at `path` (for example `"/ready"`), running all `indicators` on each request.
    ///
    /// Like [`Self::enable_health_check`], the route is mounted at the **server root** (not under URI
    /// versioning or global prefix). JSON shape follows Terminus-style summaries: `status`, `info`,
    /// `error`, `details`. Returns **503** when any indicator reports [`HealthStatus::Down`].
    pub fn enable_readiness_check<I>(mut self, path: impl Into<String>, indicators: I) -> Self
    where
        I: IntoIterator<Item = std::sync::Arc<dyn HealthIndicator>>,
    {
        self.readiness = Some((
            Self::normalize_health_path(path.into()),
            indicators.into_iter().collect(),
        ));
        self
    }

    /// Opt-in **Prometheus** scrape endpoint and default HTTP **RED** metrics (rate, errors, duration).
    ///
    /// Exposes `GET` at `path` (default `"/metrics"` if you pass an empty string) at the **server root**,
    /// not under [`Self::enable_uri_versioning`] or [`Self::set_global_prefix`]. Registers:
    /// `http_requests_total{method,status}`, `http_request_duration_seconds{method}`, `http_requests_in_flight`.
    pub fn enable_metrics(mut self, path: impl Into<String>) -> Self {
        let s = path.into();
        let p = if s.trim().is_empty() {
            "/metrics".to_string()
        } else {
            Self::normalize_health_path(s)
        };
        self.metrics_path = Some(p);
        self
    }

    /// Enables OpenAPI JSON + Swagger UI (feature: `openapi`).
    ///
    /// Registers:
    /// - `GET /openapi.json`
    /// - `GET /docs`
    #[cfg(feature = "openapi")]
    pub fn enable_openapi(mut self) -> Self {
        self.openapi = Some(nestrs_openapi::OpenApiOptions::default());
        self
    }

    #[cfg(feature = "openapi")]
    pub fn enable_openapi_with_options(mut self, options: nestrs_openapi::OpenApiOptions) -> Self {
        self.openapi = Some(options);
        self
    }

    /// Enables a GraphQL endpoint (feature: `graphql`).
    ///
    /// Registers `GET/POST /graphql` under the app router (so URI versioning/global prefix apply).
    #[cfg(feature = "graphql")]
    pub fn enable_graphql<Query, Mutation, Subscription>(
        mut self,
        schema: crate::graphql::Schema<Query, Mutation, Subscription>,
    ) -> Self
    where
        Query: crate::graphql::ObjectType + Send + Sync + 'static,
        Mutation: crate::graphql::ObjectType + Send + Sync + 'static,
        Subscription: crate::graphql::SubscriptionType + Send + Sync + 'static,
    {
        self.router = self
            .router
            .merge(crate::graphql::graphql_router(schema, "/graphql"));
        self
    }

    #[cfg(feature = "graphql")]
    pub fn enable_graphql_with_path<Query, Mutation, Subscription>(
        mut self,
        schema: crate::graphql::Schema<Query, Mutation, Subscription>,
        path: impl Into<String>,
    ) -> Self
    where
        Query: crate::graphql::ObjectType + Send + Sync + 'static,
        Mutation: crate::graphql::ObjectType + Send + Sync + 'static,
        Subscription: crate::graphql::SubscriptionType + Send + Sync + 'static,
    {
        self.router = self.router.merge(crate::graphql::graphql_router(schema, path));
        self
    }

    /// Emits structured request logs through `tracing` with fields:
    /// `method`, `path`, `status`, `duration_ms`, and `request_id` (when present).
    pub fn use_request_tracing(mut self, options: RequestTracingOptions) -> Self {
        self.request_tracing = Some(options);
        self
    }

    /// Installs the global `tracing` subscriber (see [`try_init_tracing`]). Call **once** near process startup,
    /// before [`Self::listen`], so log output and [`Self::use_request_tracing`] share the same pipeline.
    pub fn configure_tracing(self, config: TracingConfig) -> Self {
        try_init_tracing(config)
            .unwrap_or_else(|e| panic!("nestrs: configure_tracing failed: {e}"));
        self
    }

    /// Like [`Self::configure_tracing`], but also exports spans to an OTLP collector (OpenTelemetry).
    ///
    /// Feature-gated behind `otel`.
    #[cfg(feature = "otel")]
    pub fn configure_tracing_opentelemetry(
        self,
        config: TracingConfig,
        otel: OpenTelemetryConfig,
    ) -> Self {
        try_init_tracing_opentelemetry(config, otel)
            .unwrap_or_else(|e| panic!("nestrs: configure_tracing_opentelemetry failed: {e}"));
        self
    }

    /// Applies an arbitrary Tower [`axum::Router::layer`] (or other `Router` transform) **around the full app**
    /// after all built-in middleware (CORS, rate limit, request id, metrics, request tracing, request
    /// decompression, response compression, path normalization, etc.).
    ///
    /// **Order:** the **first** call is the **innermost** among your custom layers; the **last** call is the
    /// **outermost** (first to see the incoming request). This matches Axum’s “last `.layer` wins outermost” rule.
    pub fn use_global_layer<F>(mut self, apply: F) -> Self
    where
        F: Fn(axum::Router) -> axum::Router + Send + Sync + 'static,
    {
        self.global_layers.push(Box::new(apply));
        self
    }

    /// Registers a global [`ExceptionFilter`] for responses produced from [`HttpException`] (handlers returning
    /// `Err(HttpException)`, guard failures, etc.).
    ///
    /// The filter runs **inside** built-in layers such as CORS, rate limiting, and [`Self::enable_production_errors`],
    /// so it sees the original exception payload and can rewrite the response before sanitization.
    pub fn use_global_exception_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter + 'static,
    {
        self.exception_filter = Some(std::sync::Arc::new(filter));
        self
    }

    /// Installs [`nestrs_default_not_found_handler`] so requests that match no route get a JSON
    /// [`NotFoundException`] body (`Cannot METHOD /path`), consistent with handler-produced errors.
    pub fn enable_default_fallback(mut self) -> Self {
        self.default_404_fallback = true;
        self
    }

    fn normalize_health_path(path: String) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return "/health".to_string();
        }
        let inner = trimmed.trim_matches('/');
        if inner.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", inner)
        }
    }

    fn build_router(self) -> axum::Router {
        let production_errors = self.production_errors;
        let request_context = self.request_context;
        let request_scope = self.request_scope;
        let i18n = self.i18n;
        let request_id = self.request_id;
        let static_mounts = self.static_mounts;
        let liveness_path = self.liveness_path;
        let readiness = self.readiness;
        let metrics_path = self.metrics_path.clone();
        let request_tracing = self.request_tracing;
        let global_layers = self.global_layers;
        let default_404_fallback = self.default_404_fallback;
        let compression = self.compression;
        let request_decompression = self.request_decompression;
        let concurrency_limit = self.concurrency_limit;
        let load_shed = self.load_shed;
        let registry = self.registry;
        let uri_version = self.uri_version;
        let api_versioning = self.api_versioning.clone();
        let global_prefix = self.global_prefix;
        #[cfg(feature = "openapi")]
        let openapi = self.openapi;
        let mut router = self.router;

        if let Some(ref v) = uri_version {
            router = axum::Router::new().nest(v, router);
        }

        if let Some(ref p) = global_prefix {
            router = axum::Router::new().nest(p, router);
        }

        if let Some(policy) = api_versioning {
            let opts = std::sync::Arc::new(policy);
            router = router.layer(axum::middleware::from_fn_with_state(
                opts,
                crate::versioning::api_version_middleware,
            ));
        }

        for (mount_path, dir) in static_mounts {
            let svc = tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true);
            let static_router = axum::Router::new().nest_service(mount_path.as_str(), svc);
            router = axum::Router::new().merge(static_router).merge(router);
        }

        if let Some(path) = liveness_path {
            let probe = axum::Router::new().route(&path, axum::routing::get(liveness_handler));
            router = axum::Router::new().merge(probe).merge(router);
        }

        if let Some((path, indicators)) = readiness {
            let ctx = std::sync::Arc::new(ReadinessContext { indicators });
            let probe = axum::Router::new().route(
                &path,
                axum::routing::get(move || {
                    let c = ctx.clone();
                    async move { readiness_handler(c).await }
                }),
            );
            router = axum::Router::new().merge(probe).merge(router);
        }

        if let Some(ref path) = metrics_path {
            let handle = init_prometheus_recorder().clone();
            let path = path.clone();
            let probe = axum::Router::new().route(
                path.as_str(),
                axum::routing::get(move || {
                    let handle = handle.clone();
                    async move {
                        (
                            [(
                                axum::http::header::CONTENT_TYPE,
                                axum::http::HeaderValue::from_static("text/plain; version=0.0.4"),
                            )],
                            handle.render(),
                        )
                    }
                }),
            );
            router = axum::Router::new().merge(probe).merge(router);
        }

        #[cfg(feature = "openapi")]
        if let Some(mut options) = openapi {
            let api_prefix = match (global_prefix.as_deref(), uri_version.as_deref()) {
                (None, None) => "".to_string(),
                (Some(p), None) => p.to_string(),
                (None, Some(v)) => v.to_string(),
                (Some(p), Some(v)) => {
                    format!("{}/{}", p.trim_end_matches('/'), v.trim_start_matches('/'))
                }
            };
            options.api_prefix = api_prefix;
            let docs = nestrs_openapi::openapi_router(options);
            router = axum::Router::new().merge(docs).merge(router);
        }

        if default_404_fallback {
            router = router.fallback(axum::routing::any(nestrs_default_not_found_handler));
        }

        if let Some(filter) = self.exception_filter.clone() {
            router = router.layer(axum::middleware::from_fn_with_state(
                filter,
                exception_filter::exception_filter_middleware,
            ));
        }

        if request_scope {
            router = router.layer(axum::middleware::from_fn_with_state(
                registry.clone(),
                request_scope_middleware,
            ));
        }

        if let Some(cors) = self.cors_options {
            router = router.layer(cors.into_layer());
        }

        if let Some(headers) = self.security_headers {
            router = headers.apply(router);
        }

        if let Some(options) = self.rate_limit_options {
            let state = std::sync::Arc::new(RateLimitState::new(options));
            router = router.layer(axum::middleware::from_fn_with_state(
                state,
                rate_limit_middleware,
            ));
        }

        if let Some(duration) = self.request_timeout {
            router = router.layer(axum::middleware::from_fn_with_state(
                duration,
                request_timeout_middleware,
            ));
        }

        if let Some(max) = concurrency_limit {
            if load_shed {
                let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max));
                router = router.layer(axum::middleware::from_fn_with_state(
                    sem,
                    load_shed_middleware,
                ));
            } else {
                router = router.layer(tower::limit::ConcurrencyLimitLayer::new(max));
            }
        }

        if let Some(limit) = self.body_limit_bytes {
            router = router.layer(tower_http::limit::RequestBodyLimitLayer::new(limit));
        }

        if production_errors {
            router = router.layer(axum::middleware::from_fn(
                production_error_sanitize_middleware,
            ));
        }

        if request_context {
            router = router.layer(axum::middleware::from_fn(
                request_context::install_request_context_middleware,
            ));
        }

        if i18n {
            let svc = registry.get::<crate::I18nService>();
            router = router.layer(axum::middleware::from_fn_with_state(
                svc,
                i18n::install_i18n_middleware,
            ));
        }

        if request_id {
            use tower_http::request_id::{
                MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer,
            };
            // First `.layer` is innermost: Propagate wraps the router; Set wraps Propagate so the
            // request hits Set before Propagate (matches tower-http ServiceBuilder example order).
            router = router
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid));
        }

        if let Some(scrape_path) = metrics_path {
            router = router.layer(axum::middleware::from_fn_with_state(
                HttpMetricsState { scrape_path },
                http_metrics_middleware,
            ));
        }

        if let Some(options) = request_tracing {
            router = router.layer(axum::middleware::from_fn_with_state(
                options,
                request_tracing_middleware,
            ));
        }

        if request_decompression {
            router = router.layer(tower_http::decompression::RequestDecompressionLayer::new());
        }

        if compression {
            router = router.layer(tower_http::compression::CompressionLayer::new());
        }

        for apply in global_layers {
            router = apply(router);
        }

        router
    }

    /// Builds the [`axum::Router`] with all middleware except [`Self::use_path_normalization`], which is
    /// cleared here so the returned value is always a plain [`axum::Router`].
    pub fn into_router(self) -> axum::Router {
        let mut s = self;
        s.path_normalization = None;
        s.build_router()
    }

    pub async fn listen(self, port: u16) {
        let ip = self
            .listen_ip
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        let registry = self.registry.clone();
        let mut s = self;
        let path_normalization = s.path_normalization.take();
        let router = s.build_router();

        registry.eager_init_singletons();
        #[cfg(feature = "microservices")]
        crate::microservices::wire_on_event_handlers(registry.as_ref());
        registry.run_on_module_init().await;
        registry.run_on_application_bootstrap().await;
        #[cfg(feature = "schedule")]
        crate::schedule::wire_scheduled_tasks(registry.as_ref()).await;
        #[cfg(feature = "queues")]
        crate::queues::wire_queue_processors(registry.as_ref()).await;

        let listener = tokio::net::TcpListener::bind((ip, port))
            .await
            .unwrap_or_else(|e| panic!("failed to bind on {ip}:{port}: {e}"));

        axum_serve(listener, router, path_normalization, None).await;
    }

    /// Like [`Self::listen`], but stops when `shutdown` completes. Uses Axum’s graceful shutdown so
    /// in-flight requests can finish (see [`axum::serve::Serve::with_graceful_shutdown`]).
    pub async fn listen_with_shutdown<F>(self, port: u16, shutdown: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let ip = self
            .listen_ip
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        let registry = self.registry.clone();
        let mut s = self;
        let path_normalization = s.path_normalization.take();
        let router = s.build_router();

        registry.eager_init_singletons();
        #[cfg(feature = "microservices")]
        crate::microservices::wire_on_event_handlers(registry.as_ref());
        registry.run_on_module_init().await;
        registry.run_on_application_bootstrap().await;
        #[cfg(feature = "schedule")]
        crate::schedule::wire_scheduled_tasks(registry.as_ref()).await;
        #[cfg(feature = "queues")]
        crate::queues::wire_queue_processors(registry.as_ref()).await;

        let listener = tokio::net::TcpListener::bind((ip, port))
            .await
            .unwrap_or_else(|e| panic!("failed to bind on {ip}:{port}: {e}"));

        axum_serve(
            listener,
            router,
            path_normalization,
            Some(Box::pin(shutdown)),
        )
        .await;

        registry.run_on_application_shutdown().await;
        registry.run_on_module_destroy().await;
        #[cfg(feature = "otel")]
        if OTEL_INSTALLED.get().is_some() {
            crate::otel::shutdown_tracer_provider();
        }
    }

    /// [`Self::listen_with_shutdown`] with **Ctrl+C** on all platforms and **SIGTERM** on Unix
    /// (containers / process managers).
    pub async fn listen_graceful(self, port: u16) {
        self.listen_with_shutdown(port, nestrs_shutdown_signal())
            .await;
    }
}

async fn axum_serve(
    listener: tokio::net::TcpListener,
    router: axum::Router,
    path_normalization: Option<PathNormalization>,
    shutdown: Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'static>>>,
) {
    use axum::extract::Request;
    use axum::ServiceExt;
    use tower::Layer;
    use std::net::SocketAddr;

    let err = |e: std::io::Error| panic!("server error: {e}");

    match (path_normalization, shutdown) {
        (None, None) => axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap_or_else(err),
        (None, Some(s)) => axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
            .with_graceful_shutdown(s)
            .await
            .unwrap_or_else(err),
        (Some(PathNormalization::TrimTrailingSlash), None) => {
            let app =
                tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash().layer(router);
            axum::serve(
                listener,
                ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
            )
                .await
                .unwrap_or_else(err)
        }
        (Some(PathNormalization::TrimTrailingSlash), Some(s)) => {
            let app =
                tower_http::normalize_path::NormalizePathLayer::trim_trailing_slash().layer(router);
            axum::serve(
                listener,
                ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
            )
                .with_graceful_shutdown(s)
                .await
                .unwrap_or_else(err)
        }
        (Some(PathNormalization::AppendTrailingSlash), None) => {
            let app = tower_http::normalize_path::NormalizePathLayer::append_trailing_slash()
                .layer(router);
            axum::serve(
                listener,
                ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
            )
                .await
                .unwrap_or_else(err)
        }
        (Some(PathNormalization::AppendTrailingSlash), Some(s)) => {
            let app = tower_http::normalize_path::NormalizePathLayer::append_trailing_slash()
                .layer(router);
            axum::serve(
                listener,
                ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(app),
            )
                .with_graceful_shutdown(s)
                .await
                .unwrap_or_else(err)
        }
    }
}

async fn nestrs_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    tracing::info!(target: "nestrs", "shutdown signal received, draining connections");
}

impl SecurityHeaders {
    fn apply(self, mut router: axum::Router) -> axum::Router {
        if let Some(v) = self.x_content_type_options {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("x-content-type-options"),
                    axum::http::HeaderValue::from_str(&v)
                        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("nosniff")),
                ),
            );
        }
        if let Some(v) = self.x_frame_options {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("x-frame-options"),
                    axum::http::HeaderValue::from_str(&v)
                        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("DENY")),
                ),
            );
        }
        if let Some(v) = self.referrer_policy {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("referrer-policy"),
                    axum::http::HeaderValue::from_str(&v).unwrap_or_else(|_| {
                        axum::http::HeaderValue::from_static("strict-origin-when-cross-origin")
                    }),
                ),
            );
        }
        if let Some(v) = self.x_xss_protection {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("x-xss-protection"),
                    axum::http::HeaderValue::from_str(&v)
                        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("0")),
                ),
            );
        }
        if let Some(v) = self.permissions_policy {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("permissions-policy"),
                    axum::http::HeaderValue::from_str(&v).unwrap_or_else(|_| {
                        axum::http::HeaderValue::from_static(
                            "geolocation=(), microphone=(), camera=()",
                        )
                    }),
                ),
            );
        }
        if let Some(v) = self.content_security_policy {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("content-security-policy"),
                    axum::http::HeaderValue::from_str(&v).unwrap_or_else(|_| {
                        axum::http::HeaderValue::from_static("default-src 'self'")
                    }),
                ),
            );
        }
        if let Some(v) = self.hsts {
            router = router.layer(
                tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                    axum::http::header::HeaderName::from_static("strict-transport-security"),
                    axum::http::HeaderValue::from_str(&v).unwrap_or_else(|_| {
                        axum::http::HeaderValue::from_static("max-age=31536000")
                    }),
                ),
            );
        }
        router
    }
}

#[derive(Clone)]
struct HttpMetricsState {
    scrape_path: String,
}

async fn http_metrics_middleware(
    axum::extract::State(state): axum::extract::State<HttpMetricsState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = req.uri().path();
    if path == state.scrape_path {
        return next.run(req).await;
    }

    metrics::gauge!("http_requests_in_flight").increment(1.0);

    let method = req.method().as_str().to_owned();
    let started = std::time::Instant::now();

    let response = next.run(req).await;
    let status = response.status().as_u16().to_string();

    metrics::gauge!("http_requests_in_flight").decrement(1.0);
    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(),
        "status" => status,
    )
    .increment(1);
    metrics::histogram!("http_request_duration_seconds", "method" => method)
        .record(started.elapsed().as_secs_f64());

    response
}

async fn request_tracing_middleware(
    axum::extract::State(options): axum::extract::State<RequestTracingOptions>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = req.uri().path().to_string();
    if options.skip_paths.iter().any(|p| p == &path) {
        return next.run(req).await;
    }

    let method = req.method().to_string();
    let started = std::time::Instant::now();
    let response = next.run(req).await;
    let status = response.status().as_u16();
    let duration_ms = started.elapsed().as_millis() as u64;
    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    tracing::info!(
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration_ms,
        request_id = %request_id,
        "http request completed"
    );

    response
}

async fn request_scope_middleware(
    axum::extract::State(registry): axum::extract::State<std::sync::Arc<crate::core::ProviderRegistry>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    crate::core::with_request_scope(async move {
        let (mut parts, body) = req.into_parts();
        parts.extensions.insert(registry);
        let req = axum::http::Request::from_parts(parts, body);
        next.run(req).await
    })
    .await
}

#[derive(Debug)]
struct RateLimitState {
    inner: RateLimitInner,
}

#[derive(Debug)]
enum RateLimitInner {
    Memory {
        options: RateLimitOptions,
        window: tokio::sync::Mutex<RateLimitWindow>,
    },
    #[cfg(feature = "cache-redis")]
    Redis {
        client: redis::Client,
        key_prefix: String,
        window_secs: u64,
        max_requests: u64,
    },
}

#[derive(Debug)]
struct RateLimitWindow {
    started_at: std::time::Instant,
    count: u64,
}

impl RateLimitState {
    fn new(options: RateLimitOptions) -> Self {
        #[cfg(feature = "cache-redis")]
        if let Some(r) = options.redis.clone() {
            match redis::Client::open(r.url.as_str()) {
                Ok(client) => {
                    return Self {
                        inner: RateLimitInner::Redis {
                            client,
                            key_prefix: r.key_prefix,
                            window_secs: options.window_secs,
                            max_requests: options.max_requests,
                        },
                    };
                }
                Err(e) => {
                    tracing::warn!(target: "nestrs", "redis rate limit: invalid URL: {e}");
                }
            }
        }

        Self {
            inner: RateLimitInner::Memory {
                options,
                window: tokio::sync::Mutex::new(RateLimitWindow {
                    started_at: std::time::Instant::now(),
                    count: 0,
                }),
            },
        }
    }
}

async fn rate_limit_middleware(
    axum::extract::State(state): axum::extract::State<std::sync::Arc<RateLimitState>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    match &state.inner {
        RateLimitInner::Memory { options, window } => {
            let mut guard = window.lock().await;
            if guard.started_at.elapsed().as_secs() >= options.window_secs {
                guard.started_at = std::time::Instant::now();
                guard.count = 0;
            }
            if guard.count >= options.max_requests {
                return axum::response::IntoResponse::into_response(TooManyRequestsException::new(
                    "Rate limit exceeded",
                ));
            }
            guard.count += 1;
        }
        #[cfg(feature = "cache-redis")]
        RateLimitInner::Redis {
            client,
            key_prefix,
            window_secs,
            max_requests,
        } => {
            let ip = client_ip_from_request(&req);
            let key = format!("{key_prefix}:{ip}");
            match redis_rate_allow(client, &key, *window_secs, *max_requests).await {
                Ok(true) => {}
                Ok(false) => {
                    return axum::response::IntoResponse::into_response(TooManyRequestsException::new(
                        "Rate limit exceeded",
                    ));
                }
                Err(e) => {
                    tracing::warn!(target: "nestrs", "redis rate limit check failed: {e}");
                }
            }
        }
    }
    next.run(req).await
}

#[cfg(feature = "cache-redis")]
async fn redis_rate_allow(
    client: &redis::Client,
    key: &str,
    window_secs: u64,
    max_requests: u64,
) -> Result<bool, redis::RedisError> {
    let mut conn = client.get_multiplexed_tokio_connection().await?;
    let count: i64 = redis::cmd("INCR").arg(key).query_async(&mut conn).await?;
    let count = u64::try_from(count).unwrap_or(0);
    if count == 1 {
        redis::cmd("EXPIRE")
            .arg(key)
            .arg(window_secs as usize)
            .query_async::<()>(&mut conn)
            .await
            .ok();
    }
    Ok(count <= max_requests)
}

#[cfg(feature = "cache-redis")]
fn client_ip_from_request(req: &axum::extract::Request) -> String {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next().map(str::trim))
        .map(|s| s.to_string())
        .or_else(|| {
            req.extensions()
                .get::<crate::ClientIp>()
                .map(|c| c.0.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

async fn request_timeout_middleware(
    axum::extract::State(duration): axum::extract::State<std::time::Duration>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    match tokio::time::timeout(duration, next.run(req)).await {
        Ok(response) => response,
        Err(_) => axum::response::IntoResponse::into_response(GatewayTimeoutException::new(
            "Request timed out",
        )),
    }
}

async fn load_shed_middleware(
    axum::extract::State(semaphore): axum::extract::State<std::sync::Arc<tokio::sync::Semaphore>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    match semaphore.clone().try_acquire_owned() {
        Ok(_permit) => next.run(req).await,
        Err(_) => axum::response::IntoResponse::into_response(ServiceUnavailableException::new(
            "Server overloaded",
        )),
    }
}

/// JSON **404** for unmatched routes; used when [`NestApplication::enable_default_fallback`] is set.
pub async fn nestrs_default_not_found_handler(
    req: axum::extract::Request,
) -> axum::response::Response {
    let method = req.method().as_str();
    let path = req.uri().path();
    axum::response::IntoResponse::into_response(NotFoundException::new(format!(
        "Cannot {method} {path}"
    )))
}

async fn liveness_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

async fn readiness_handler(
    ctx: std::sync::Arc<ReadinessContext>,
) -> impl axum::response::IntoResponse {
    use axum::http::StatusCode;

    let mut info = serde_json::Map::new();
    let mut err = serde_json::Map::new();

    for ind in ctx.indicators() {
        match ind.check().await {
            HealthStatus::Up => {
                info.insert(
                    ind.name().to_string(),
                    serde_json::json!({ "status": "up" }),
                );
            }
            HealthStatus::Down { message } => {
                err.insert(
                    ind.name().to_string(),
                    serde_json::json!({ "status": "down", "message": message }),
                );
            }
        }
    }

    let body = if err.is_empty() {
        serde_json::json!({
            "status": "ok",
            "info": info,
            "error": {},
            "details": {},
        })
    } else {
        serde_json::json!({
            "status": "error",
            "info": info,
            "error": err,
            "details": {},
        })
    };

    let status = if err.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, axum::Json(body))
}

/// Strips internal detail from Nest-shaped JSON error bodies for 5xx responses when
/// `enable_production_errors` is set on `NestApplication`.
async fn production_error_sanitize_middleware(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let res = next.run(req).await;
    let status = res.status();
    if !status.is_server_error() {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let ctype = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ctype.starts_with("application/json") {
        return axum::response::Response::from_parts(parts, body);
    }
    let Ok(bytes) = to_bytes(body, 256 * 1024).await else {
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        return axum::response::Response::from_parts(parts, Body::empty());
    };
    let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        return axum::response::Response::from_parts(parts, Body::from(bytes));
    };
    let Some(obj) = val.as_object_mut() else {
        parts.headers.remove(axum::http::header::CONTENT_LENGTH);
        return axum::response::Response::from_parts(parts, Body::from(bytes));
    };
    obj.insert(
        "message".to_string(),
        serde_json::json!("An unexpected error occurred"),
    );
    if !obj.contains_key("error") {
        obj.insert(
            "error".to_string(),
            serde_json::json!(status.canonical_reason().unwrap_or("Internal Server Error")),
        );
    }
    obj.remove("errors");
    let new_body = match serde_json::to_vec(&val) {
        Ok(b) => b,
        Err(_) => {
            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
            return axum::response::Response::from_parts(parts, Body::from(bytes));
        }
    };
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    axum::response::Response::from_parts(parts, Body::from(new_body))
}

#[derive(Debug, Clone)]
pub struct HttpException {
    pub status: axum::http::StatusCode,
    pub message: String,
    pub error: String,
    pub details: Option<serde_json::Value>,
}

impl HttpException {
    pub fn new(
        status: axum::http::StatusCode,
        message: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            status,
            message: message.into(),
            error: error.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

pub struct BadRequestException;

impl BadRequestException {
    #[allow(clippy::new_ret_no_self)] // Nest-style factory: returns HttpException, not Self
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(axum::http::StatusCode::BAD_REQUEST, message, "Bad Request")
    }
}

macro_rules! define_http_exception {
    ($name:ident, $status:expr, $label:literal) => {
        pub struct $name;
        impl $name {
            #[allow(clippy::new_ret_no_self)] // Nest-style factory: returns HttpException, not Self
            pub fn new(message: impl Into<String>) -> HttpException {
                HttpException::new($status, message, $label)
            }
        }
    };
}

define_http_exception!(
    UnauthorizedException,
    axum::http::StatusCode::UNAUTHORIZED,
    "Unauthorized"
);
define_http_exception!(
    PaymentRequiredException,
    axum::http::StatusCode::PAYMENT_REQUIRED,
    "Payment Required"
);
define_http_exception!(
    ForbiddenException,
    axum::http::StatusCode::FORBIDDEN,
    "Forbidden"
);
define_http_exception!(
    NotFoundException,
    axum::http::StatusCode::NOT_FOUND,
    "Not Found"
);
define_http_exception!(
    MethodNotAllowedException,
    axum::http::StatusCode::METHOD_NOT_ALLOWED,
    "Method Not Allowed"
);
define_http_exception!(
    NotAcceptableException,
    axum::http::StatusCode::NOT_ACCEPTABLE,
    "Not Acceptable"
);
define_http_exception!(
    RequestTimeoutException,
    axum::http::StatusCode::REQUEST_TIMEOUT,
    "Request Timeout"
);
define_http_exception!(
    ConflictException,
    axum::http::StatusCode::CONFLICT,
    "Conflict"
);
define_http_exception!(GoneException, axum::http::StatusCode::GONE, "Gone");
define_http_exception!(
    PayloadTooLargeException,
    axum::http::StatusCode::PAYLOAD_TOO_LARGE,
    "Payload Too Large"
);
define_http_exception!(
    UnsupportedMediaTypeException,
    axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
    "Unsupported Media Type"
);
define_http_exception!(
    UnprocessableEntityException,
    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
    "Unprocessable Entity"
);
define_http_exception!(
    TooManyRequestsException,
    axum::http::StatusCode::TOO_MANY_REQUESTS,
    "Too Many Requests"
);
define_http_exception!(
    InternalServerErrorException,
    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    "Internal Server Error"
);
define_http_exception!(
    NotImplementedException,
    axum::http::StatusCode::NOT_IMPLEMENTED,
    "Not Implemented"
);
define_http_exception!(
    BadGatewayException,
    axum::http::StatusCode::BAD_GATEWAY,
    "Bad Gateway"
);
define_http_exception!(
    ServiceUnavailableException,
    axum::http::StatusCode::SERVICE_UNAVAILABLE,
    "Service Unavailable"
);
define_http_exception!(
    GatewayTimeoutException,
    axum::http::StatusCode::GATEWAY_TIMEOUT,
    "Gateway Timeout"
);

impl From<core::GuardError> for HttpException {
    fn from(value: core::GuardError) -> Self {
        match value {
            core::GuardError::Unauthorized(m) => UnauthorizedException::new(m),
            core::GuardError::Forbidden(m) => ForbiddenException::new(m),
        }
    }
}

impl axum::response::IntoResponse for HttpException {
    fn into_response(self) -> axum::response::Response {
        use axum::http::header::CONTENT_TYPE;
        let status = self.status;
        let mut payload = serde_json::json!({
            "statusCode": status.as_u16(),
            "message": &self.message,
            "error": &self.error,
        });
        if let Some(ref details) = self.details {
            payload["errors"] = details.clone();
        }
        let body = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(_) => br#"{"statusCode":500,"message":"Serialization failed","error":"Internal Server Error"}"#.to_vec(),
        };
        let mut res = axum::response::Response::new(Body::from(body));
        *res.status_mut() = status;
        res.headers_mut().insert(
            CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        res.extensions_mut().insert(self);
        res
    }
}

fn __nestrs_validation_failed(e: validator::ValidationErrors) -> HttpException {
    let mut errors = Vec::new();
    for (field, field_errors) in e.field_errors() {
        let constraints = field_errors
            .iter()
            .map(|ve| {
                let code = ve.code.to_string();
                let message = ve
                    .message
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| code.clone());
                (code, message)
            })
            .collect::<std::collections::HashMap<_, _>>();

        errors.push(serde_json::json!({
            "field": field,
            "constraints": constraints,
        }));
    }

    UnprocessableEntityException::new("Validation failed").with_details(serde_json::json!(errors))
}

pub struct ValidatedBody<T>(pub T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequest<S> for ValidatedBody<T>
where
    S: Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = HttpException;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) =
            <axum::Json<T> as axum::extract::FromRequest<S>>::from_request(req, state)
                .await
                .map_err(|e| BadRequestException::new(format!("Invalid JSON body: {}", e)))?;

        value.validate().map_err(__nestrs_validation_failed)?;

        Ok(Self(value))
    }
}

/// Validates a query string-extracted DTO (NestJS `ValidationPipe` + `@Query()` analogue).
pub struct ValidatedQuery<T>(pub T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = HttpException;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) =
            <axum::extract::Query<T> as axum::extract::FromRequestParts<S>>::from_request_parts(
                parts, state,
            )
            .await
            .map_err(|e| BadRequestException::new(format!("Invalid query: {}", e)))?;
        value.validate().map_err(__nestrs_validation_failed)?;
        Ok(Self(value))
    }
}

/// Validates a path param-extracted DTO (NestJS `ValidationPipe` + `@Param()` analogue).
pub struct ValidatedPath<T>(pub T);

#[axum::async_trait]
impl<S, T> axum::extract::FromRequestParts<S> for ValidatedPath<T>
where
    S: Send + Sync + 'static,
    T: serde::de::DeserializeOwned + Validate + Send + 'static,
{
    type Rejection = HttpException;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Path(value) =
            <axum::extract::Path<T> as axum::extract::FromRequestParts<S>>::from_request_parts(
                parts, state,
            )
            .await
            .map_err(|e| BadRequestException::new(format!("Invalid path params: {}", e)))?;
        value.validate().map_err(__nestrs_validation_failed)?;
        Ok(Self(value))
    }
}

/// Used by [`impl_routes!`] for each guard type; not stable API.
#[doc(hidden)]
pub async fn __nestrs_run_guard<G>(
    parts: &::axum::http::request::Parts,
) -> Result<(), crate::core::GuardError>
where
    G: crate::core::CanActivate + Default,
{
    G::default().can_activate(parts).await
}

/// Registers HTTP routes for a `#[controller]` type. Each line: `METHOD "path" with (RouteGuards...) => Handler,`.
/// Use `with ()` when a route has no route-level guards. Route guards run **inside** (after) controller guards.
///
/// Optional **controller** guard (one type; compose multiple checks inside that type if needed):
/// `impl_routes!(MyCtl, state S, controller_guards(MyCtrlGuard) => [ ... ])`
#[macro_export]
macro_rules! impl_routes {
    (
        $controller:ty, state $state_ty:ty => [
            $(
                $(@ver($route_version:literal))?
                $method:ident $path:literal
                with ( $($guard:ty),* )
                $( interceptors ( $($interceptor:ty),* ) )?
                $( filters ( $($filter:ty),* ) )?
                $( metadata ( $( ( $meta_key:literal, $meta_value:literal ) ),* ) )?
                => $handler:path
                ,
            )+
        ]
    ) => {
        impl $crate::core::Controller for $controller {
            fn register(
                router: axum::Router,
                registry: &$crate::core::ProviderRegistry
            ) -> axum::Router {
                let state = registry.get::<$state_ty>();
                let prefix = <$controller>::__nestrs_prefix();
                let version = <$controller>::__nestrs_version();
                let __nestrs_router = router
                    $(
                        .route(
                            {
                                let __path = $crate::impl_routes!(
                                    @join
                                    $crate::impl_routes!(
                                        @effective_version
                                        version
                                        $(, $route_version)?
                                    ),
                                    prefix,
                                    $path
                                );
                                $crate::core::RouteRegistry::register(
                                    stringify!($method),
                                    __path,
                                    concat!(module_path!(), "::", stringify!($handler)),
                                );
                                __path
                            },
                            {
                                $(
                                    $(
                                        $crate::core::MetadataRegistry::set(
                                            concat!(module_path!(), "::", stringify!($handler)),
                                            $meta_key,
                                            $meta_value,
                                        );
                                    )*
                                )?
                                let mut __route = $crate::impl_routes!(@method $method, $handler);
                                __route = $crate::impl_routes!(
                                    @apply_interceptors
                                    __route
                                    $(, $($interceptor),* )?
                                );
                                __route = __route.layer(::axum::middleware::from_fn(
                                    |req: ::axum::extract::Request,
                                     next: ::axum::middleware::Next| async move {
                                        let (mut parts, body) = req.into_parts();
                                        parts.extensions.insert($crate::core::HandlerKey(
                                            concat!(module_path!(), "::", stringify!($handler)),
                                        ));
                                        $(
                                            if let Err(e) =
                                                $crate::__nestrs_run_guard::<$guard>(&parts).await
                                            {
                                                return ::axum::response::IntoResponse::into_response(e);
                                            }
                                        )*
                                        let req = ::axum::http::Request::from_parts(parts, body);
                                        next.run(req).await
                                    },
                                ));
                                __route = $crate::impl_routes!(
                                    @apply_filters
                                    __route
                                    $(, $($filter),* )?
                                );
                                __route.with_state(state.clone())
                            }
                        )
                    )+;
                $crate::impl_routes!(@maybe_host_wrap $controller, __nestrs_router)
            }
        }
    };
    (
        $controller:ty, state $state_ty:ty,
        controller_guards ( $ctrl_guard:ty )
        => [
            $(
                $(@ver($route_version:literal))?
                $method:ident $path:literal
                with ( $($guard:ty),* )
                $( interceptors ( $($interceptor:ty),* ) )?
                $( filters ( $($filter:ty),* ) )?
                $( metadata ( $( ( $meta_key:literal, $meta_value:literal ) ),* ) )?
                => $handler:path
                ,
            )+
        ]
    ) => {
        impl $crate::core::Controller for $controller {
            fn register(
                router: axum::Router,
                registry: &$crate::core::ProviderRegistry
            ) -> axum::Router {
                let state = registry.get::<$state_ty>();
                let prefix = <$controller>::__nestrs_prefix();
                let version = <$controller>::__nestrs_version();
                let __nestrs_router = router
                    $(
                        .route(
                            {
                                let __path = $crate::impl_routes!(
                                    @join
                                    $crate::impl_routes!(
                                        @effective_version
                                        version
                                        $(, $route_version)?
                                    ),
                                    prefix,
                                    $path
                                );
                                $crate::core::RouteRegistry::register(
                                    stringify!($method),
                                    __path,
                                    concat!(module_path!(), "::", stringify!($handler)),
                                );
                                __path
                            },
                            {
                                $(
                                    $(
                                        $crate::core::MetadataRegistry::set(
                                            concat!(module_path!(), "::", stringify!($handler)),
                                            $meta_key,
                                            $meta_value,
                                        );
                                    )*
                                )?
                                let mut __route = $crate::impl_routes!(@method $method, $handler);
                                __route = $crate::impl_routes!(
                                    @apply_interceptors
                                    __route
                                    $(, $($interceptor),* )?
                                );
                                __route = __route.layer(::axum::middleware::from_fn(
                                    |req: ::axum::extract::Request,
                                     next: ::axum::middleware::Next| async move {
                                        let (mut parts, body) = req.into_parts();
                                        parts.extensions.insert($crate::core::HandlerKey(
                                            concat!(module_path!(), "::", stringify!($handler)),
                                        ));
                                        $(
                                            if let Err(e) =
                                                $crate::__nestrs_run_guard::<$guard>(&parts).await
                                            {
                                                return ::axum::response::IntoResponse::into_response(e);
                                            }
                                        )*
                                        let req = ::axum::http::Request::from_parts(parts, body);
                                        next.run(req).await
                                    },
                                ));
                                __route = __route.layer(::axum::middleware::from_fn(
                                    |req: ::axum::extract::Request,
                                     next: ::axum::middleware::Next| async move {
                                        let (mut parts, body) = req.into_parts();
                                        parts.extensions.insert($crate::core::HandlerKey(
                                            concat!(module_path!(), "::", stringify!($handler)),
                                        ));
                                        if let Err(e) =
                                            $crate::__nestrs_run_guard::<$ctrl_guard>(&parts).await
                                        {
                                            return ::axum::response::IntoResponse::into_response(e);
                                        }
                                        let req = ::axum::http::Request::from_parts(parts, body);
                                        next.run(req).await
                                    },
                                ));
                                __route = $crate::impl_routes!(
                                    @apply_filters
                                    __route
                                    $(, $($filter),* )?
                                );
                                __route.with_state(state.clone())
                            }
                        )
                    )+;
                $crate::impl_routes!(@maybe_host_wrap $controller, __nestrs_router)
            }
        }
    };
    (@maybe_host_wrap $controller:ty, $inner:expr) => {
        match <$controller>::__nestrs_host() {
            None => $inner,
            Some(__nestrs_h) => $inner.layer(::axum::middleware::from_fn_with_state(
                __nestrs_h,
                $crate::host_restriction_middleware,
            )),
        }
    };
    (@effective_version $controller_version:expr) => { $controller_version };
    (@effective_version $controller_version:expr, $route_version:literal) => { $route_version };
    (@join $version:expr, $prefix:expr, $path:literal) => {{
        let v = $version.trim_matches('/');
        let mut p = $prefix.trim_end_matches('/');
        let s = $path;

        if !p.is_empty() && !p.starts_with('/') {
            p = std::boxed::Box::leak(format!("/{}", p).into_boxed_str());
        }

        let base = if p.is_empty() || p == "/" {
            if s.starts_with('/') {
                s.to_string()
            } else {
                format!("/{}", s)
            }
        } else if s == "/" {
            p.to_string()
        } else {
            format!("{}/{}", p, s.trim_start_matches('/'))
        };
        let joined = if v.is_empty() {
            base
        } else if base == "/" {
            format!("/{}", v)
        } else {
            format!("/{}/{}", v, base.trim_start_matches('/'))
        };
        std::boxed::Box::leak(joined.into_boxed_str())
    }};
    (@method GET, $handler:path) => { axum::routing::get($handler) };
    (@method POST, $handler:path) => { axum::routing::post($handler) };
    (@method PUT, $handler:path) => { axum::routing::put($handler) };
    (@method PATCH, $handler:path) => { axum::routing::patch($handler) };
    (@method DELETE, $handler:path) => { axum::routing::delete($handler) };
    (@method OPTIONS, $handler:path) => { axum::routing::options($handler) };
    (@method HEAD, $handler:path) => { axum::routing::head($handler) };
    (@method ALL, $handler:path) => { axum::routing::any($handler) };
    (@apply_interceptors $router:expr) => { $router };
    (@apply_interceptors $router:expr,) => { $router };
    (@apply_interceptors $router:expr, $head:ty $(, $tail:ty)*) => {{
        $crate::impl_routes!(@apply_interceptors $router $(, $tail)*)
            .layer($crate::interceptor_layer!($head))
    }};
    (@apply_filters $router:expr) => { $router };
    (@apply_filters $router:expr,) => { $router };
    (@apply_filters $router:expr, $head:ty $(, $tail:ty)*) => {{
        $crate::impl_routes!(@apply_filters $router $(, $tail)*)
            .layer(::axum::middleware::from_fn(
                |req: ::axum::extract::Request, next: ::axum::middleware::Next| async move {
                    let filter: $head = ::core::default::Default::default();
                    let res = next.run(req).await;
                    if let Some(ex) = res.extensions().get::<$crate::HttpException>().cloned() {
                        filter.catch(ex).await
                    } else {
                        res
                    }
                },
            ))
    }};
}
