//! Composite metrics export: nestrs owns the single global `metrics`
//! recorder slot with a fanout, and forwards every instrument registration
//! and recording to each registered backend.
//!
//! The `metrics` facade admits exactly one global recorder. nestrs claims
//! that slot — on behalf of *all* backends — with [`NestMetricsRecorder`]:
//! backends register themselves as members via [`add_recorder_member`] and
//! every subsequent `metrics::counter!` / `gauge!` / `histogram!` call
//! fans out to all of them.
//!
//! Backends that ship with nestrs:
//!
//! - the Prometheus recorder (built via `PrometheusBuilder::build_recorder`,
//!   rendered at `/metrics`) — installed by `enable_metrics`, and
//! - the OTLP bridge (behind the `otel` feature, see [`otel_bridge`]) which
//!   forwards the same instruments to the configured OpenTelemetry
//!   `MeterProvider` — installed by `OpenTelemetryConfig::metrics()`.
//!
//! Members can be added in any order: whichever backend initializes first
//! installs the fanout globally via [`ensure_global_composite`]; later
//! backends simply join it. An app that enables both gets Prometheus pull
//! **and** OTLP push from one recording surface — including its own
//! user-defined `metrics::*!` instruments.

use std::sync::{Arc, OnceLock, RwLock};

use metrics::{
    set_global_recorder, Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName,
    Metadata, Recorder, SharedString, Unit,
};

type MemberVec = Arc<RwLock<Vec<Arc<dyn Recorder + Send + Sync>>>>;

fn members() -> &'static MemberVec {
    static MEMBERS: OnceLock<MemberVec> = OnceLock::new();
    MEMBERS.get_or_init(|| Arc::new(RwLock::new(Vec::new())))
}

/// Installs [`NestMetricsRecorder`] as the process-global `metrics`
/// recorder, unless nestrs already owns the slot.
///
/// Panics if a *foreign* recorder was installed globally first (the same
/// conflict semantics the standalone Prometheus installer had).
pub(crate) fn ensure_global_composite() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    if INSTALLED.set(()).is_ok() {
        set_global_recorder(NestMetricsRecorder::new())
            .expect("nestrs: failed to install the nestrs metrics recorder (another recorder was already installed globally)");
    }
}

/// Registers a backend recorder as a fanout member. Order-independent:
/// members added after the fanout is installed are picked up by the next
/// recording call.
pub(crate) fn add_recorder_member(member: Arc<dyn Recorder + Send + Sync>) {
    members()
        .write()
        .expect("nestrs: metrics members lock poisoned")
        .push(member);
}

/// Declares the framework's own RED metrics (name, unit, description) so
/// every backend renders them with metadata from the very first scrape or
/// export. Idempotent; called by `enable_metrics` and the OTLP-metrics path.
pub(crate) fn describe_framework_metrics() {
    metrics::describe_counter!(
        "http_requests_total",
        Unit::Count,
        "HTTP requests handled, by method and status code."
    );
    metrics::describe_gauge!(
        "http_requests_in_flight",
        Unit::Count,
        "HTTP requests currently being served."
    );
    metrics::describe_histogram!(
        "http_request_duration_seconds",
        Unit::Seconds,
        "HTTP request latency in seconds."
    );
}

/// The nestrs-owned global `metrics` recorder: a fanout to every
/// registered backend member.
///
/// Each `register_*` call asks every member for *its* handle for the key
/// (members maintain their own per-key caches) and returns one composite
/// handle that fans recordings out to all of them. Members are snapshotted
/// per call, so backends that join later are included immediately.
pub(crate) struct NestMetricsRecorder {
    members: MemberVec,
}

impl NestMetricsRecorder {
    fn new() -> Self {
        Self {
            members: members().clone(),
        }
    }

    #[cfg(test)]
    fn with_members(members: Vec<Arc<dyn Recorder + Send + Sync>>) -> Self {
        Self {
            members: Arc::new(RwLock::new(members)),
        }
    }

    fn snapshot(&self) -> Vec<Arc<dyn Recorder + Send + Sync>> {
        self.members
            .read()
            .expect("nestrs: metrics members lock poisoned")
            .clone()
    }
}

impl Recorder for NestMetricsRecorder {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        for member in self.snapshot() {
            member.describe_counter(key.clone(), unit, description.clone());
        }
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        for member in self.snapshot() {
            member.describe_gauge(key.clone(), unit, description.clone());
        }
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        for member in self.snapshot() {
            member.describe_histogram(key.clone(), unit, description.clone());
        }
    }

    fn register_counter(&self, key: &Key, metadata: &Metadata<'_>) -> Counter {
        let handles: Vec<Counter> = self
            .snapshot()
            .iter()
            .map(|member| member.register_counter(key, metadata))
            .collect();
        Counter::from_arc(Arc::new(FanoutCounter(handles)))
    }

    fn register_gauge(&self, key: &Key, metadata: &Metadata<'_>) -> Gauge {
        let handles: Vec<Gauge> = self
            .snapshot()
            .iter()
            .map(|member| member.register_gauge(key, metadata))
            .collect();
        Gauge::from_arc(Arc::new(FanoutGauge(handles)))
    }

    fn register_histogram(&self, key: &Key, metadata: &Metadata<'_>) -> Histogram {
        let handles: Vec<Histogram> = self
            .snapshot()
            .iter()
            .map(|member| member.register_histogram(key, metadata))
            .collect();
        Histogram::from_arc(Arc::new(FanoutHistogram(handles)))
    }
}

struct FanoutCounter(Vec<Counter>);

impl CounterFn for FanoutCounter {
    fn increment(&self, value: u64) {
        for handle in &self.0 {
            handle.increment(value);
        }
    }

    fn absolute(&self, value: u64) {
        for handle in &self.0 {
            handle.absolute(value);
        }
    }
}

struct FanoutGauge(Vec<Gauge>);

impl GaugeFn for FanoutGauge {
    fn increment(&self, value: f64) {
        for handle in &self.0 {
            handle.increment(value);
        }
    }

    fn decrement(&self, value: f64) {
        for handle in &self.0 {
            handle.decrement(value);
        }
    }

    fn set(&self, value: f64) {
        for handle in &self.0 {
            handle.set(value);
        }
    }
}

struct FanoutHistogram(Vec<Histogram>);

impl HistogramFn for FanoutHistogram {
    fn record(&self, value: f64) {
        for handle in &self.0 {
            handle.record(value);
        }
    }
}

/// Bridges the `metrics` facade to an OpenTelemetry `MeterProvider`.
///
/// Every facade instrument that is registered while this bridge is a
/// fanout member becomes one OTel instrument (created lazily, cached per
/// metric name) whose per-label-set series map to OTel attributes:
/// `counter!("rps", "route" => "/x")` exports as the OTel counter `rps`
/// with attribute `route="/x"`.
///
/// Semantic conversions:
///
/// - **counters** — facade `increment` maps to OTel `add`; facade
///   `absolute(v)` maps to `add(v - last)` (monotonic counters cannot go
///   backwards, so decreases are clamped to zero).
/// - **gauges** — OTel gauges are absolute-only, so facade
///   `increment`/`decrement` are accumulated per label set and the running
///   total is recorded.
/// - **histograms** — record 1:1; OTel bucket boundaries are controlled by
///   collector-side views, not by this bridge.
///
/// Metadata from `metrics::describe_*!` macros (unit, description) is
/// captured per metric name and applied to the OTel instrument when it is
/// first built. Describing after the first recording for a name does not
/// retroactively annotate it — the same limitation as the Prometheus
/// exporter, and the framework metrics are always described before use.
#[cfg(feature = "otel")]
pub mod otel_bridge;

#[cfg(feature = "otel")]
pub use otel_bridge::OtelMetricsBridge;

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::Level;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A recorder member that records into shared storage, used to prove the
    /// fanout forwards registrations and recordings to every member.
    #[derive(Default)]
    struct CapturingInner {
        counters: Mutex<HashMap<String, u64>>,
        gauges: Mutex<HashMap<String, f64>>,
        histograms: Mutex<HashMap<String, (u64, f64)>>,
    }

    // The `Recorder` methods only see `&self`, so the capture handles keep
    // their own `Arc` to the shared storage.
    #[derive(Clone, Default)]
    struct Capturing(Arc<CapturingInner>);

    struct CaptureCounter {
        shared: Arc<CapturingInner>,
        name: String,
    }

    impl CounterFn for CaptureCounter {
        fn increment(&self, value: u64) {
            *self
                .shared
                .counters
                .lock()
                .unwrap()
                .entry(self.name.clone())
                .or_insert(0) += value;
        }

        fn absolute(&self, value: u64) {
            self.shared
                .counters
                .lock()
                .unwrap()
                .insert(self.name.clone(), value);
        }
    }

    struct CaptureGauge {
        shared: Arc<CapturingInner>,
        name: String,
    }

    impl GaugeFn for CaptureGauge {
        fn increment(&self, value: f64) {
            *self
                .shared
                .gauges
                .lock()
                .unwrap()
                .entry(self.name.clone())
                .or_insert(0.0) += value;
        }

        fn decrement(&self, value: f64) {
            *self
                .shared
                .gauges
                .lock()
                .unwrap()
                .entry(self.name.clone())
                .or_insert(0.0) -= value;
        }

        fn set(&self, value: f64) {
            self.shared
                .gauges
                .lock()
                .unwrap()
                .insert(self.name.clone(), value);
        }
    }

    struct CaptureHistogram {
        shared: Arc<CapturingInner>,
        name: String,
    }

    impl HistogramFn for CaptureHistogram {
        fn record(&self, value: f64) {
            let mut histograms = self.shared.histograms.lock().unwrap();
            let entry = histograms.entry(self.name.clone()).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += value;
        }
    }

    impl Recorder for Capturing {
        fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

        fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
            Counter::from_arc(Arc::new(CaptureCounter {
                shared: self.0.clone(),
                name: key.name().to_string(),
            }))
        }

        fn register_gauge(&self, key: &Key, _: &Metadata<'_>) -> Gauge {
            Gauge::from_arc(Arc::new(CaptureGauge {
                shared: self.0.clone(),
                name: key.name().to_string(),
            }))
        }

        fn register_histogram(&self, key: &Key, _: &Metadata<'_>) -> Histogram {
            Histogram::from_arc(Arc::new(CaptureHistogram {
                shared: self.0.clone(),
                name: key.name().to_string(),
            }))
        }
    }

    #[test]
    fn fanout_forwards_to_all_members() {
        let m1 = Capturing::default();
        let m2 = Capturing::default();
        let fanout = NestMetricsRecorder::with_members(vec![
            Arc::new(m1.clone()) as Arc<dyn Recorder + Send + Sync>,
            Arc::new(m2.clone()),
        ]);

        let key = Key::from_parts("fanout_total", vec![metrics::Label::new("m", "1")]);
        let metadata = Metadata::new("test", Level::INFO, None);
        fanout.register_counter(&key, &metadata).increment(3);
        fanout.register_counter(&key, &metadata).increment(4);

        assert_eq!(m1.0.counters.lock().unwrap().get("fanout_total"), Some(&7));
        assert_eq!(m2.0.counters.lock().unwrap().get("fanout_total"), Some(&7));
    }

    #[test]
    fn fanout_forwards_gauges_and_histograms() {
        let m1 = Capturing::default();
        let fanout = NestMetricsRecorder::with_members(vec![
            Arc::new(m1.clone()) as Arc<dyn Recorder + Send + Sync>
        ]);

        let metadata = Metadata::new("test", Level::INFO, None);
        let gauge = fanout.register_gauge(&Key::from_name("g"), &metadata);
        gauge.increment(2.0);
        gauge.decrement(0.5);
        let histogram = fanout.register_histogram(&Key::from_name("h"), &metadata);
        histogram.record(0.25);

        assert_eq!(m1.0.gauges.lock().unwrap().get("g"), Some(&1.5));
        assert_eq!(m1.0.histograms.lock().unwrap().get("h"), Some(&(1, 0.25)));
    }
}

#[cfg(all(test, feature = "otel"))]
mod otel_tests {
    use super::otel_bridge::OtelMetricsBridge;
    use metrics::{Level, Metadata, Recorder};
    use opentelemetry::metrics::MeterProvider;
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};

    #[test]
    fn bridge_exports_counter_gauge_histogram() {
        let exporter = InMemoryMetricExporter::default();
        let provider = SdkMeterProvider::builder()
            .with_reader(PeriodicReader::builder(exporter.clone()).build())
            .build();
        let meter = provider.meter("nestrs-test");
        let bridge = OtelMetricsBridge::new(meter);

        // Describe first — same order the framework guarantees for its own
        // RED metrics.
        bridge.describe_counter(
            "bridge_total".into(),
            Some(metrics::Unit::Count),
            "test counter".into(),
        );
        let metadata = Metadata::new("test", Level::INFO, None);

        let counter = bridge.register_counter(
            &metrics::Key::from_parts("bridge_total", vec![metrics::Label::new("route", "/x")]),
            &metadata,
        );
        counter.increment(2);
        counter.increment(5);

        let gauge = bridge.register_gauge(&metrics::Key::from_name("bridge_gauge"), &metadata);
        gauge.increment(2.0);
        gauge.decrement(0.5);

        let histogram =
            bridge.register_histogram(&metrics::Key::from_name("bridge_duration"), &metadata);
        histogram.record(0.25);

        provider.force_flush().unwrap();
        let finished = exporter.get_finished_metrics().unwrap();
        assert!(!finished.is_empty(), "expected at least one export");

        let mut counter_value = None;
        let mut counter_attrs = Vec::new();
        let mut gauge_value = None;
        let mut histogram_sum = None;
        for rm in &finished {
            for scope in rm.scope_metrics() {
                for metric in scope.metrics() {
                    match (metric.name(), metric.data()) {
                        ("bridge_total", AggregatedMetrics::U64(MetricData::Sum(sum))) => {
                            let dp = sum.data_points().next().expect("counter data point");
                            counter_value = Some(dp.value());
                            counter_attrs = dp.attributes().cloned().collect();
                        }
                        ("bridge_gauge", AggregatedMetrics::F64(MetricData::Gauge(gauge))) => {
                            let dp = gauge.data_points().next().expect("gauge data point");
                            gauge_value = Some(dp.value());
                        }
                        (
                            "bridge_duration",
                            AggregatedMetrics::F64(MetricData::Histogram(hist)),
                        ) => {
                            let dp = hist.data_points().next().expect("histogram data point");
                            histogram_sum = Some(dp.sum());
                        }
                        _ => {}
                    }
                }
            }
        }

        assert_eq!(counter_value, Some(7), "counter increments must add");
        assert!(
            counter_attrs
                .iter()
                .any(|kv| kv.key.as_str() == "route" && kv.value.as_str() == "/x"),
            "facade labels must become OTel attributes, got {counter_attrs:?}"
        );
        assert_eq!(gauge_value, Some(1.5), "gauge deltas must net to absolute");
        assert_eq!(histogram_sum, Some(0.25), "histogram must record value");
    }
}
