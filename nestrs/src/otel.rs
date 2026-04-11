#![cfg(feature = "otel")]

use opentelemetry::global;
use opentelemetry::KeyValue;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::resource::Resource;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use std::sync::OnceLock;
use std::time::Duration;

static OTEL_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

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

// --- Log correlation (traces + logs) ---
//
// Logs emitted with `tracing::info!` / `tracing::error!` inside an active span share the same trace
// context as OTLP traces when using `tracing-opentelemetry`. For **OTLP log** export, route
// structured logs through the OpenTelemetry Collector (e.g. `filelog` receiver → OTLP) or adopt
// an ecosystem crate when stable Rust OTLP log exporters match your `opentelemetry` version.
