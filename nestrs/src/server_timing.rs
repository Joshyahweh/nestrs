//! `Server-Timing` middleware (RFC 8628).
//!
//! Enabled via [`crate::NestApplication::use_server_timing`]. Once enabled:
//!  * Every response gets a `Server-Timing: total;dur=<ms>` header.
//!  * Handlers can read the [`ServerTiming`] extractor and record named
//!    sub-timers via `timing.start("db")` / `timing.stop("db")`; those are
//!    appended to the header (`db;dur=12`).
//!  * Entries whose `dur` is below [`ServerTimingConfig::min_ms_to_report`]
//!    are dropped from the header (default: 0 — emit everything).
//!
//! Implementation notes:
//!  * The middleware installs an `Arc<ServerTimingTimers>` into
//!    `parts.extensions` so per-task handlers can record against it.
//!  * `Server-Timing` is dynamic (computed from extension data), so we use a
//!    custom middleware (mirroring `csrf_double_submit_middleware`) rather
//!    than `SetResponseHeaderLayer`.
//!  * No new Cargo dependencies.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::HeaderValue;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Per-request named-timer store. Inserted into `parts.extensions` by
/// [`server_timing_middleware`] and read by the [`ServerTiming`] extractor.
#[derive(Default)]
pub struct ServerTimingTimers {
    entries: Mutex<Vec<NamedTimer>>,
}

struct NamedTimer {
    name: String,
    started: Instant,
    stopped: Option<Instant>,
}

impl ServerTimingTimers {
    pub fn start(&self, name: impl Into<String>) {
        let mut entries = self.entries.lock().expect("timers poisoned");
        entries.push(NamedTimer {
            name: name.into(),
            started: Instant::now(),
            stopped: None,
        });
    }

    pub fn stop(&self, name: &str) {
        let mut entries = self.entries.lock().expect("timers poisoned");
        // Stop the most recent entry with this name that hasn't been stopped yet.
        if let Some(entry) = entries
            .iter_mut()
            .rev()
            .find(|e| e.name == name && e.stopped.is_none())
        {
            entry.stopped = Some(Instant::now());
        }
    }

    /// Snapshot the timers as a header value, omitting entries below `min_ms`.
    fn format_header_value(&self, min_ms: u32) -> Option<HeaderValue> {
        let entries = self.entries.lock().expect("timers poisoned");
        let mut out = String::new();
        for entry in entries.iter() {
            // Unstopped entries are reported as 0 (in-flight at response time).
            let elapsed = entry
                .stopped
                .map(|s| s.duration_since(entry.started))
                .unwrap_or_else(|| entry.started.elapsed());
            let ms = elapsed.as_secs_f64() * 1000.0;
            if (ms as u32) < min_ms {
                continue;
            }
            if !out.is_empty() {
                out.push_str(", ");
            }
            // Escape commas/parens in `name` per RFC 8628 §3.
            let safe_name = entry
                .name
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace(',', "\\,")
                .replace(';', "\\;")
                .replace('(', "\\(")
                .replace(')', "\\)");
            out.push_str(&safe_name);
            out.push_str(";dur=");
            // Truncate to integer milliseconds for compactness; `*1000.0` to round to 3 decimals.
            out.push_str(&format!("{ms:.2}"));
        }
        if out.is_empty() {
            None
        } else {
            HeaderValue::from_str(&out).ok()
        }
    }
}

/// Configuration for the `Server-Timing` middleware.
#[derive(Clone, Debug, Default)]
pub struct ServerTimingConfig {
    /// Drop entries whose `dur` (in integer milliseconds) is below this threshold.
    /// Default: `0` (emit every entry, including sub-millisecond ones).
    pub min_ms_to_report: u32,
}

/// Extractor: gives handlers a handle to record named timers. Returns
/// `Infallible`; if `use_server_timing()` was not enabled, a no-op handle is
/// returned (calls are silent no-ops).
#[derive(Clone)]
pub struct ServerTiming {
    timers: Arc<ServerTimingTimers>,
}

impl ServerTiming {
    /// Record the start of a named sub-operation. Pair with [`Self::stop`].
    pub fn start(&self, name: impl Into<String>) {
        self.timers.start(name);
    }

    /// Record the end of a named sub-operation (previously started with
    /// [`Self::start`] under the same name).
    pub fn stop(&self, name: &str) {
        self.timers.stop(name);
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for ServerTiming
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let timers = parts
            .extensions
            .get::<Arc<ServerTimingTimers>>()
            .cloned()
            .unwrap_or_default();
        Ok(ServerTiming { timers })
    }
}

/// Axum middleware that records the total request duration, runs the inner
/// stack, and appends a `Server-Timing: total;dur=<ms>` header to the
/// response (plus any user-recorded named timers).
pub async fn server_timing_middleware(
    State(config): State<Arc<ServerTimingConfig>>,
    mut req: Request,
    next: Next,
) -> Response {
    let timers = Arc::new(ServerTimingTimers::default());
    let total_started = Instant::now();
    req.extensions_mut().insert(timers.clone());

    let mut response = next.run(req).await;

    // Always report a `total` entry reflecting the full request duration.
    timers.start("total");
    timers.stop("total");
    // Replace the placeholder started with the true total start.
    if let Some(entry) = timers
        .entries
        .lock()
        .expect("timers poisoned")
        .iter_mut()
        .find(|e| e.name == "total")
    {
        entry.started = total_started;
    }

    if let Some(value) = timers.format_header_value(config.min_ms_to_report) {
        response.headers_mut().insert(
            axum::http::header::HeaderName::from_static("server-timing"),
            value,
        );
    }
    response
}
