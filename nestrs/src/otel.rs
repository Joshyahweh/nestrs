use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::resource::Resource;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use std::sync::OnceLock;
use std::time::Duration;

static OTEL_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();
static OTEL_METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();
static OTEL_LOGGER_PROVIDER: OnceLock<opentelemetry_sdk::logs::SdkLoggerProvider> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OtlpProtocol {
    Grpc,
}

#[derive(Clone, Debug)]
pub struct OpenTelemetryConfig {
    service_name: String,
    endpoint: Option<String>,
    protocol: OtlpProtocol,
    sample_ratio: f64,
    timeout: Option<Duration>,
    resource_attributes: Vec<(String, String)>,
    metrics: bool,
    logs: bool,
}

impl OpenTelemetryConfig {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            endpoint: None,
            protocol: OtlpProtocol::Grpc,
            sample_ratio: 1.0,
            timeout: None,
            resource_attributes: Vec::new(),
            metrics: false,
            logs: false,
        }
    }

    /// Override OTLP endpoint (default: `OTEL_EXPORTER_OTLP_ENDPOINT` or `http://localhost:4317`).
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Sampling ratio in \([0.0, 1.0]\). Default: `1.0` (always sample).
    pub fn sample_ratio(mut self, ratio: f64) -> Self {
        self.sample_ratio = ratio;
        self
    }

    /// Export timeout (transport-specific). When unset, exporter defaults apply.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Adds a resource attribute (e.g. `"deployment.environment" = "prod"`).
    pub fn resource_attr(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.resource_attributes.push((key.into(), value.into()));
        self
    }

    /// Also export **metrics** over OTLP. Every `metrics::counter!` /
    /// `gauge!` / `histogram!` instrument — the framework's RED metrics and
    /// your own — fans out to the OTLP pipeline in addition to (or instead
    /// of, if [`NestApplication::enable_metrics`](crate::NestApplication::enable_metrics)
    /// is not called) the Prometheus `/metrics` endpoint. Composable with
    /// it: enabling both gives Prometheus pull and OTLP push from one
    /// recording surface.
    pub fn metrics(mut self) -> Self {
        self.metrics = true;
        self
    }

    /// Also export **logs** over OTLP: every `tracing::info!` /
    /// `error!` / ... event additionally becomes an OpenTelemetry log
    /// record on the same OTLP pipeline as traces (including trace/span
    /// correlation for events emitted inside an active span).
    pub fn logs(mut self) -> Self {
        self.logs = true;
        self
    }

    fn resolved_endpoint(&self) -> String {
        self.endpoint
            .clone()
            .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "http://localhost:4317".to_string())
    }

    fn resolved_sampler(&self) -> Sampler {
        let ratio = self.sample_ratio.clamp(0.0, 1.0);
        Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(ratio)))
    }

    fn resource(&self) -> Resource {
        let mut b = Resource::builder().with_service_name(self.service_name.clone());
        for (k, v) in &self.resource_attributes {
            b = b.with_attribute(KeyValue::new(k.clone(), v.clone()));
        }
        b.build()
    }
}

pub fn install_otlp_tracer(
    config: OpenTelemetryConfig,
) -> Result<opentelemetry_sdk::trace::Tracer, String> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    let endpoint = config.resolved_endpoint();
    let resource = config.resource();
    let sampler = config.resolved_sampler();

    let exporter = match config.protocol {
        OtlpProtocol::Grpc => {
            let mut builder = SpanExporter::builder().with_tonic();
            builder = builder.with_endpoint(endpoint);
            if let Some(timeout) = config.timeout {
                builder = builder.with_timeout(timeout);
            }
            builder.build().map_err(|e| e.to_string())?
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(sampler)
        .with_batch_exporter(exporter)
        .build();

    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "nestrs");
    let _ = OTEL_PROVIDER.set(provider.clone());
    global::set_tracer_provider(provider);
    Ok(tracer)
}

pub fn shutdown_tracer_provider() {
    if let Some(provider) = OTEL_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

/// Whether the config opted into OTLP metrics export (via
/// [`OpenTelemetryConfig::metrics`]).
pub(crate) fn wants_metrics(config: &OpenTelemetryConfig) -> bool {
    config.metrics
}

/// Whether the config opted into OTLP log export (via
/// [`OpenTelemetryConfig::logs`]).
pub(crate) fn wants_logs(config: &OpenTelemetryConfig) -> bool {
    config.logs
}

/// Builds the OTLP metric pipeline (exporter + `PeriodicReader` +
/// `SdkMeterProvider`), installs it as the global meter provider, and
/// returns the `nestrs` meter the facade bridge records through.
///
/// The `PeriodicReader` exports on its own schedule (default: every 60s,
/// overridable via `OTEL_METRIC_EXPORT_INTERVAL`) from a dedicated thread —
/// exports themselves do not need a Tokio runtime. Construction does:
/// the tonic channel is lazy-spawned onto the **current** Tokio reactor,
/// so call this from async context (e.g. `#[tokio::main]` before
/// `listen()`) — the same requirement [`install_otlp_tracer`] has.
pub fn install_otlp_meter(
    config: &OpenTelemetryConfig,
) -> Result<opentelemetry::metrics::Meter, String> {
    let endpoint = config.resolved_endpoint();
    let resource = config.resource();

    let mut builder = opentelemetry_otlp::MetricExporter::builder().with_tonic();
    builder = builder.with_endpoint(endpoint);
    if let Some(timeout) = config.timeout {
        builder = builder.with_timeout(timeout);
    }
    let exporter = builder.build().map_err(|e| e.to_string())?;

    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter).build();
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();
    let meter = opentelemetry::metrics::MeterProvider::meter(&provider, "nestrs");
    let _ = OTEL_METER_PROVIDER.set(provider.clone());
    global::set_meter_provider(provider);
    Ok(meter)
}

/// Builds the OTLP log pipeline (exporter + batching
/// `SdkLoggerProvider`), stores it for shutdown, and returns the provider
/// to bridge into the `tracing` subscriber (via
/// `opentelemetry_appender_tracing::OpenTelemetryTracingBridge`).
///
/// Log export runs on a dedicated batch thread. Like
/// [`install_otlp_meter`], construction requires a current Tokio reactor
/// (lazy tonic channel); call from async context.
pub fn install_otlp_logger(
    config: &OpenTelemetryConfig,
) -> Result<opentelemetry_sdk::logs::SdkLoggerProvider, String> {
    let endpoint = config.resolved_endpoint();
    let resource = config.resource();

    let mut builder = opentelemetry_otlp::LogExporter::builder().with_tonic();
    builder = builder.with_endpoint(endpoint);
    if let Some(timeout) = config.timeout {
        builder = builder.with_timeout(timeout);
    }
    let exporter = builder.build().map_err(|e| e.to_string())?;

    let provider = opentelemetry_sdk::logs::SdkLoggerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let _ = OTEL_LOGGER_PROVIDER.set(provider.clone());
    Ok(provider)
}

/// Flushes and stops the OTLP metric pipeline (no-op when metrics were
/// never installed). Blocks up to the exporter timeout.
pub fn shutdown_meter_provider() {
    if let Some(provider) = OTEL_METER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

/// Flushes and stops the OTLP log pipeline (no-op when logs were never
/// installed). Bounded by the SDK's internal export timeout.
pub fn shutdown_logger_provider() {
    if let Some(provider) = OTEL_LOGGER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}
