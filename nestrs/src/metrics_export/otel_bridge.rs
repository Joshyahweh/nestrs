//! The OTLP metrics bridge — see the [parent module](super) for how it is
//! composed into the fanout recorder.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use metrics::{
    Counter, CounterFn, Gauge, GaugeFn, Histogram, HistogramFn, Key, KeyName, Metadata, Recorder,
    SharedString, Unit,
};
use opentelemetry::metrics::{
    Counter as OtelCounter, Gauge as OtelGauge, Histogram as OtelHistogram, Meter,
};
use opentelemetry::KeyValue;

/// Metadata declared via `metrics::describe_*!`, applied to the OTel
/// instrument when it is first built for a metric name.
#[derive(Default, Clone)]
struct MetricMeta {
    unit: Option<Unit>,
    description: Option<SharedString>,
}

/// Maps a facade `Unit` to an OTel unit string (UCUM-style), as OTel
/// semantic conventions require.
fn otel_unit(unit: &Unit) -> &'static str {
    match unit {
        Unit::Seconds => "s",
        Unit::Count => "1",
        Unit::Percent => "%",
        Unit::Bytes => "By",
        other => other.as_str(),
    }
}

/// Converts facade labels into OTel attributes.
fn attributes(key: &Key) -> Vec<KeyValue> {
    key.labels()
        .map(|label| KeyValue::new(label.key().to_owned(), label.value().to_owned()))
        .collect()
}

/// The bridge itself. Created from the app's OTel `Meter` (see
/// `install_otlp_meter`) and registered as a fanout member.
pub struct OtelMetricsBridge {
    meter: Meter,
    described: Mutex<HashMap<String, MetricMeta>>,
    counter_instruments: Mutex<HashMap<String, OtelCounter<u64>>>,
    gauge_instruments: Mutex<HashMap<String, OtelGauge<f64>>>,
    histogram_instruments: Mutex<HashMap<String, OtelHistogram<f64>>>,
    // Per-`Key` (name + label set) facade handles. The `metrics` facade has
    // no per-callsite caching: `counter!(...)` calls `register_counter` on
    // every invocation, so these maps are the hot path and mirror the
    // per-key caching the Prometheus recorder does internally.
    counter_handles: Mutex<HashMap<Key, Counter>>,
    gauge_handles: Mutex<HashMap<Key, Gauge>>,
    histogram_handles: Mutex<HashMap<Key, Histogram>>,
}

impl OtelMetricsBridge {
    pub fn new(meter: Meter) -> Self {
        Self {
            meter,
            described: Mutex::new(HashMap::new()),
            counter_instruments: Mutex::new(HashMap::new()),
            gauge_instruments: Mutex::new(HashMap::new()),
            histogram_instruments: Mutex::new(HashMap::new()),
            counter_handles: Mutex::new(HashMap::new()),
            gauge_handles: Mutex::new(HashMap::new()),
            histogram_handles: Mutex::new(HashMap::new()),
        }
    }

    fn meta_for(&self, name: &str) -> MetricMeta {
        self.described
            .lock()
            .expect("nestrs: OTel bridge describe lock poisoned")
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    fn counter_instrument(&self, name: &str) -> OtelCounter<u64> {
        let mut instruments = self
            .counter_instruments
            .lock()
            .expect("nestrs: OTel bridge counter lock poisoned");
        if let Some(instrument) = instruments.get(name) {
            return instrument.clone();
        }
        let meta = self.meta_for(name);
        let mut builder = self.meter.u64_counter(name.to_string());
        if let Some(unit) = &meta.unit {
            builder = builder.with_unit(otel_unit(unit));
        }
        if let Some(description) = &meta.description {
            builder = builder.with_description(description.to_string());
        }
        let instrument = builder.build();
        instruments.insert(name.to_string(), instrument.clone());
        instrument
    }

    fn gauge_instrument(&self, name: &str) -> OtelGauge<f64> {
        let mut instruments = self
            .gauge_instruments
            .lock()
            .expect("nestrs: OTel bridge gauge lock poisoned");
        if let Some(instrument) = instruments.get(name) {
            return instrument.clone();
        }
        let meta = self.meta_for(name);
        let mut builder = self.meter.f64_gauge(name.to_string());
        if let Some(unit) = &meta.unit {
            builder = builder.with_unit(otel_unit(unit));
        }
        if let Some(description) = &meta.description {
            builder = builder.with_description(description.to_string());
        }
        let instrument = builder.build();
        instruments.insert(name.to_string(), instrument.clone());
        instrument
    }

    fn histogram_instrument(&self, name: &str) -> OtelHistogram<f64> {
        let mut instruments = self
            .histogram_instruments
            .lock()
            .expect("nestrs: OTel bridge histogram lock poisoned");
        if let Some(instrument) = instruments.get(name) {
            return instrument.clone();
        }
        let meta = self.meta_for(name);
        let mut builder = self.meter.f64_histogram(name.to_string());
        if let Some(unit) = &meta.unit {
            builder = builder.with_unit(otel_unit(unit));
        }
        if let Some(description) = &meta.description {
            builder = builder.with_description(description.to_string());
        }
        let instrument = builder.build();
        instruments.insert(name.to_string(), instrument.clone());
        instrument
    }
}

impl Recorder for OtelMetricsBridge {
    fn describe_counter(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.described
            .lock()
            .expect("nestrs: OTel bridge describe lock poisoned")
            .insert(
                key.as_str().to_string(),
                MetricMeta {
                    unit,
                    description: Some(description),
                },
            );
    }

    fn describe_gauge(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.describe_counter(key, unit, description);
    }

    fn describe_histogram(&self, key: KeyName, unit: Option<Unit>, description: SharedString) {
        self.describe_counter(key, unit, description);
    }

    fn register_counter(&self, key: &Key, _metadata: &Metadata<'_>) -> Counter {
        {
            let handles = self
                .counter_handles
                .lock()
                .expect("nestrs: OTel bridge counter handles lock poisoned");
            if let Some(handle) = handles.get(key) {
                return handle.clone();
            }
        }
        let handle = Counter::from_arc(Arc::new(BridgeCounter {
            instrument: self.counter_instrument(key.name()),
            attributes: attributes(key),
            last_absolute: AtomicU64::new(0),
        }));
        self.counter_handles
            .lock()
            .expect("nestrs: OTel bridge counter handles lock poisoned")
            .insert(key.clone(), handle.clone());
        handle
    }

    fn register_gauge(&self, key: &Key, _metadata: &Metadata<'_>) -> Gauge {
        {
            let handles = self
                .gauge_handles
                .lock()
                .expect("nestrs: OTel bridge gauge handles lock poisoned");
            if let Some(handle) = handles.get(key) {
                return handle.clone();
            }
        }
        let handle = Gauge::from_arc(Arc::new(BridgeGauge {
            instrument: self.gauge_instrument(key.name()),
            attributes: attributes(key),
            current: Mutex::new(0.0),
        }));
        self.gauge_handles
            .lock()
            .expect("nestrs: OTel bridge gauge handles lock poisoned")
            .insert(key.clone(), handle.clone());
        handle
    }

    fn register_histogram(&self, key: &Key, _metadata: &Metadata<'_>) -> Histogram {
        {
            let handles = self
                .histogram_handles
                .lock()
                .expect("nestrs: OTel bridge histogram handles lock poisoned");
            if let Some(handle) = handles.get(key) {
                return handle.clone();
            }
        }
        let handle = Histogram::from_arc(Arc::new(BridgeHistogram {
            instrument: self.histogram_instrument(key.name()),
            attributes: attributes(key),
        }));
        self.histogram_handles
            .lock()
            .expect("nestrs: OTel bridge histogram handles lock poisoned")
            .insert(key.clone(), handle.clone());
        handle
    }
}

/// Facade counter handle backed by a monotonic OTel `Counter<u64>`.
///
/// `increment` maps 1:1; `absolute(v)` maps to `add(v - last)` with
/// decreases clamped to zero (monotonic counters cannot go backwards).
struct BridgeCounter {
    instrument: OtelCounter<u64>,
    attributes: Vec<KeyValue>,
    last_absolute: AtomicU64,
}

impl CounterFn for BridgeCounter {
    fn increment(&self, value: u64) {
        self.instrument.add(value, &self.attributes);
    }

    fn absolute(&self, value: u64) {
        let previous = self.last_absolute.swap(value, Ordering::Relaxed);
        if value > previous {
            self.instrument.add(value - previous, &self.attributes);
        }
    }
}

/// Facade gauge handle backed by an absolute-only OTel `Gauge<f64>`.
///
/// The facade allows `increment`/`decrement`; OTel gauges only take
/// absolute values, so deltas are accumulated per label set and the
/// running total is recorded (mirroring the Prometheus recorder's gauge
/// semantics).
struct BridgeGauge {
    instrument: OtelGauge<f64>,
    attributes: Vec<KeyValue>,
    current: Mutex<f64>,
}

impl GaugeFn for BridgeGauge {
    fn increment(&self, value: f64) {
        let mut current = self.current.lock().expect("nestrs: gauge lock poisoned");
        *current += value;
        self.instrument.record(*current, &self.attributes);
    }

    fn decrement(&self, value: f64) {
        let mut current = self.current.lock().expect("nestrs: gauge lock poisoned");
        *current -= value;
        self.instrument.record(*current, &self.attributes);
    }

    fn set(&self, value: f64) {
        *self.current.lock().expect("nestrs: gauge lock poisoned") = value;
        self.instrument.record(value, &self.attributes);
    }
}

/// Facade histogram handle backed by an OTel `Histogram<f64>` — records
/// 1:1. Bucket boundaries are collector-side (OTel views), not exporter-side.
struct BridgeHistogram {
    instrument: OtelHistogram<f64>,
    attributes: Vec<KeyValue>,
}

impl HistogramFn for BridgeHistogram {
    fn record(&self, value: f64) {
        self.instrument.record(value, &self.attributes);
    }
}
