//! Route throttling (`#[throttle(n, "per")]` / `#[skip_throttle]`) — NestJS
//! [`@nestjs/throttler`](https://docs.nestjs.com/security/rate-limiting) parity.
//!
//! Two enforcement points:
//!
//! - **Global middleware** ([`throttler_middleware`], enabled with
//!   [`crate::NestApplication::use_throttler`]) — runs before auth guards and
//!   can attach `Retry-After` / `X-RateLimit-*` response headers on 429.
//! - **[`ThrottlerGuard`]** — a regular [`CanActivate`] guard for parity with
//!   Nest's `ThrottlerGuard` shape (rejects with
//!   [`GuardError::TooManyRequests`]).
//!
//! Per-route decorators are stored in the [`crate::core::MetadataRegistry`]
//! via the standard decorator pipeline: `#[throttle(5, "minute")]` records
//! `"throttle" => "5/minute"`, `#[skip_throttle]` records
//! `"skip_throttle" => "true"`. A route with `#[throttle]` overrides the
//! global spec; `#[skip_throttle]` exempts the route entirely.

use crate::core::{DynamicModule, Injectable, MetadataRegistry, ProviderRegistry};
use axum::extract::Request;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::Arc;

/// Fixed request budget over a sliding-start window (`"5/minute"` ⇒ 5 per 60s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottleSpec {
    pub limit: u64,
    pub window_secs: u64,
}

impl ThrottleSpec {
    /// Parse the decorator value emitted by `#[throttle(n, "per")]`.
    pub fn parse(s: &str) -> Option<Self> {
        let (limit, per) = s.split_once('/')?;
        let limit: u64 = limit.trim().parse().ok()?;
        let window_secs = match per.trim() {
            "second" => 1,
            "minute" => 60,
            "hour" => 3600,
            _ => return None,
        };
        if limit == 0 {
            return None;
        }
        Some(Self { limit, window_secs })
    }
}

/// Result of one throttle check against a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleOutcome {
    Allowed {
        /// Requests still available in the current window (including this one).
        remaining: u64,
    },
    Limited {
        /// Seconds until the window resets (for `Retry-After`).
        retry_after_secs: u64,
    },
}

/// Storage backend for throttle counters (Nest `ThrottlerStorage` analogue).
#[nestrs::async_trait]
pub trait ThrottlerBackend: Send + Sync + 'static {
    async fn check(&self, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome;
}

const THROTTLER_SHARDS: usize = 32;
/// Upper bound on tracked keys per shard between prune passes (mirror of the
/// global rate limiter's cap; sheds *new* keys when full rather than growing
/// without bound under IP rotation).
const THROTTLER_MAX_KEYS_PER_SHARD: usize = 16_384;

#[derive(Debug)]
struct ThrottleWindow {
    started_at: std::time::Instant,
    count: u64,
}

#[derive(Debug, Default)]
struct ThrottleShard {
    windows: HashMap<String, ThrottleWindow>,
    last_pruned_at: Option<std::time::Instant>,
}

impl ThrottleShard {
    fn prune_expired(&mut self, now: std::time::Instant, window_secs: u64) {
        let due = match self.last_pruned_at {
            Some(last) => now.duration_since(last).as_secs() >= window_secs,
            None => true,
        };
        if !due {
            return;
        }
        self.last_pruned_at = Some(now);
        self.windows
            .retain(|_, w| now.duration_since(w.started_at).as_secs() < window_secs);
    }
}

/// In-process, sharded fixed-window backend (the default). 32 shards spread
/// mutex contention; a panic in one request must not poison the limiter
/// (mirrors the global rate limiter's poison-tolerant locking).
#[derive(Debug, Default)]
pub struct InMemoryThrottler {
    shards: Vec<std::sync::Mutex<ThrottleShard>>,
}

impl InMemoryThrottler {
    pub fn new() -> Self {
        Self {
            shards: (0..THROTTLER_SHARDS)
                .map(|_| std::sync::Mutex::new(ThrottleShard::default()))
                .collect(),
        }
    }

    fn shard_for(&self, key: &str) -> &std::sync::Mutex<ThrottleShard> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        let idx = (hasher.finish() as usize) % self.shards.len();
        &self.shards[idx]
    }
}

#[nestrs::async_trait]
impl ThrottlerBackend for InMemoryThrottler {
    async fn check(&self, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome {
        let mut guard = match self.shard_for(key).lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let now = std::time::Instant::now();
        guard.prune_expired(now, spec.window_secs);

        if !guard.windows.contains_key(key) && guard.windows.len() >= THROTTLER_MAX_KEYS_PER_SHARD {
            guard.last_pruned_at = Some(now - std::time::Duration::from_secs(spec.window_secs));
            guard.prune_expired(now, spec.window_secs);
            if guard.windows.len() >= THROTTLER_MAX_KEYS_PER_SHARD {
                return ThrottleOutcome::Limited {
                    retry_after_secs: spec.window_secs,
                };
            }
        }

        let window = guard
            .windows
            .entry(key.to_string())
            .or_insert_with(|| ThrottleWindow {
                started_at: now,
                count: 0,
            });
        if now.duration_since(window.started_at).as_secs() >= spec.window_secs {
            window.started_at = now;
            window.count = 0;
        }
        if window.count >= spec.limit {
            let elapsed = now.duration_since(window.started_at).as_secs();
            return ThrottleOutcome::Limited {
                retry_after_secs: spec.window_secs.saturating_sub(elapsed).max(1),
            };
        }
        window.count += 1;
        ThrottleOutcome::Allowed {
            remaining: spec.limit - window.count,
        }
    }
}

/// Atomic fixed-window check returning `{count, ttl}` so `Retry-After` is
/// exact; the TTL heal covers keys left without an expiry by a crash
/// between INCR and EXPIRE (same script shape as the global rate limiter).
#[cfg(feature = "cache-redis")]
const THROTTLER_LUA: &str = r"
local count = redis.call('INCR', KEYS[1])
if count == 1 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
elseif redis.call('TTL', KEYS[1]) < 0 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
end
return {count, redis.call('TTL', KEYS[1])}
";

/// Cross-process backend: counters in Redis, keyed `{prefix}:{key}`.
/// Two service instances sharing a URL + prefix share a budget.
#[cfg(feature = "cache-redis")]
#[derive(Debug)]
pub struct RedisThrottler {
    client: redis::Client,
    key_prefix: String,
}

#[cfg(feature = "cache-redis")]
impl RedisThrottler {
    pub fn new(url: &str, key_prefix: impl Into<String>) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: redis::Client::open(url)?,
            key_prefix: key_prefix.into(),
        })
    }
}

#[cfg(feature = "cache-redis")]
#[nestrs::async_trait]
impl ThrottlerBackend for RedisThrottler {
    async fn check(&self, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome {
        let full_key = format!("{}:{}", self.key_prefix, key);
        let Ok(conn) = self.client.get_multiplexed_tokio_connection().await else {
            // Fail open on backend unavailability (same policy as the
            // global Redis rate limiter): throttling is an optimization,
            // not an availability gate.
            tracing::warn!(target: "nestrs", "redis throttler: connection failed; allowing request");
            return ThrottleOutcome::Allowed { remaining: 0 };
        };
        let result: Result<(i64, i64), redis::RedisError> = redis::cmd("EVAL")
            .arg(THROTTLER_LUA)
            .arg(1)
            .arg(&full_key)
            .arg(spec.window_secs)
            .query_async(&mut conn.clone())
            .await;
        match result {
            Ok((count, ttl)) => {
                let count = u64::try_from(count).unwrap_or(0);
                if count <= spec.limit {
                    ThrottleOutcome::Allowed {
                        remaining: spec.limit - count,
                    }
                } else {
                    let ttl = u64::try_from(ttl).unwrap_or(0);
                    ThrottleOutcome::Limited {
                        retry_after_secs: ttl.max(1),
                    }
                }
            }
            Err(e) => {
                tracing::warn!(target: "nestrs", "redis throttler check failed: {e}");
                ThrottleOutcome::Allowed { remaining: 0 }
            }
        }
    }
}

/// Backend selection for [`ThrottlerOptions`].
#[derive(Debug, Clone, Default)]
pub enum ThrottlerBackendKind {
    #[default]
    InMemory,
    /// Cross-process counters in Redis (requires the `cache-redis` feature).
    #[cfg(feature = "cache-redis")]
    Redis { url: String, key_prefix: String },
}

/// Options for [`ThrottlerModule::register`] / [`crate::NestApplication::use_throttler`].
#[derive(Debug, Clone, Default)]
pub struct ThrottlerOptions {
    /// Fallback spec applied to routes without `#[throttle(...)]`.
    /// `None` means only explicitly decorated routes are throttled.
    pub global: Option<ThrottleSpec>,
    pub backend: ThrottlerBackendKind,
    /// Forwarded-header trust level for client-IP resolution. `None` (default)
    /// inherits the application-wide hop count from
    /// [`crate::NestApplication::use_trusted_proxy_headers`], so the throttler
    /// and the `ClientIp` extractor resolve the same client identity;
    /// `Some(hops)` overrides it (forwarded headers untrusted when `Some(0)`).
    pub trusted_proxy_hops: Option<u16>,
}

/// Injectable facade over the chosen backend.
pub struct ThrottlerService {
    backend: Arc<dyn ThrottlerBackend>,
}

impl ThrottlerService {
    pub fn from_options(options: &ThrottlerOptions) -> Self {
        let backend: Arc<dyn ThrottlerBackend> = match &options.backend {
            ThrottlerBackendKind::InMemory => Arc::new(InMemoryThrottler::new()),
            #[cfg(feature = "cache-redis")]
            ThrottlerBackendKind::Redis { url, key_prefix } => {
                match RedisThrottler::new(url, key_prefix) {
                    Ok(r) => Arc::new(r),
                    Err(e) => {
                        tracing::warn!(
                            target: "nestrs",
                            "redis throttler: invalid URL ({e}); falling back to in-memory"
                        );
                        Arc::new(InMemoryThrottler::new())
                    }
                }
            }
        };
        Self { backend }
    }

    pub async fn check(&self, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome {
        self.backend.check(key, spec).await
    }
}

#[nestrs::async_trait]
impl Injectable for ThrottlerService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::from_options(&ThrottlerOptions::default()))
    }
}

/// Throttler module (exporting [`ThrottlerService`]).
pub struct ThrottlerModule;

impl ThrottlerModule {
    /// Build the module with explicit options (backend + global spec).
    pub fn register(options: ThrottlerOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        let service = Arc::new(ThrottlerService::from_options(&options));
        registry.override_provider::<ThrottlerService>(service);
        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![std::any::TypeId::of::<ThrottlerService>()],
        )
    }
}

/// Resolve the throttle spec for a request: `None` = pass through.
fn throttle_spec_for(handler: Option<&str>, global: Option<ThrottleSpec>) -> Option<ThrottleSpec> {
    match handler {
        Some(handler) => {
            if MetadataRegistry::get(handler, "skip_throttle").is_some() {
                return None;
            }
            match MetadataRegistry::get(handler, "throttle")
                .as_deref()
                .and_then(ThrottleSpec::parse)
            {
                Some(spec) => Some(spec),
                // Undecorated route: only the global spec applies.
                None => global,
            }
        }
        // No registered handler (non-route middleware path): apply global.
        None => global,
    }
}

/// Global throttling middleware (installed by
/// [`crate::NestApplication::use_throttler`]). Runs outermost — cheapest
/// check first — and short-circuits with a 429 carrying `Retry-After` /
/// `X-RateLimit-Remaining` when the budget is exhausted.
pub async fn throttler_middleware(
    axum::extract::State(state): axum::extract::State<Arc<ThrottlerState>>,
    req: Request,
    next: Next,
) -> Response {
    let (method, path) = (req.method().as_str(), req.uri().path());
    let handler = crate::core::RouteRegistry::handler_for(method, path);

    // Per-route metadata wins over the global spec; `#[skip_throttle]`
    // exempts the route entirely; non-route paths (no registered handler)
    // fall back to the global spec only.
    let spec = match handler.as_deref() {
        Some(h) if MetadataRegistry::get(h, "skip_throttle").is_some() => {
            return next.run(req).await;
        }
        Some(h) => throttle_spec_for(Some(h), state.global),
        None => state.global,
    };
    let Some(spec) = spec else {
        return next.run(req).await;
    };
    enforce(
        &state,
        handler.as_deref().unwrap_or("global"),
        spec,
        req,
        next,
    )
    .await
}

async fn enforce(
    state: &ThrottlerState,
    scope: &str,
    spec: ThrottleSpec,
    req: Request,
    next: Next,
) -> Response {
    let ip = crate::client_ip::rate_limit_key_ip(
        req.headers(),
        req.extensions(),
        Some(state.trusted_proxy_hops),
    );
    let key = format!("{scope}:{ip}");
    match state.service.check(&key, &spec).await {
        ThrottleOutcome::Allowed { .. } => next.run(req).await,
        ThrottleOutcome::Limited { retry_after_secs } => too_many_response(
            &format!("ThrottlerException: retry after {retry_after_secs}s"),
            retry_after_secs,
            spec,
        ),
    }
}

fn too_many_response(message: &str, retry_after_secs: u64, spec: ThrottleSpec) -> Response {
    let body = axum::Json(serde_json::json!({
        "statusCode": 429,
        "message": message,
        "error": "Too Many Requests",
    }));
    let mut resp = (axum::http::StatusCode::TOO_MANY_REQUESTS, body).into_response();
    let headers = resp.headers_mut();
    if let Ok(v) = retry_after_secs.to_string().parse() {
        headers.insert("retry-after", v);
    }
    headers.insert(
        "x-ratelimit-limit",
        spec.limit.to_string().parse().expect("static"),
    );
    headers.insert("x-ratelimit-remaining", "0".parse().expect("static"));
    resp
}

/// Shared middleware state (service + global spec + proxy hops).
#[derive(Clone)]
pub struct ThrottlerState {
    service: Arc<ThrottlerService>,
    global: Option<ThrottleSpec>,
    trusted_proxy_hops: u16,
}

impl ThrottlerState {
    pub fn new(options: &ThrottlerOptions) -> Self {
        Self {
            service: Arc::new(ThrottlerService::from_options(options)),
            global: options.global,
            // `build_router` resolves app-level inheritance into the options
            // before calling this; standalone users default to trusting no
            // forwarded headers (connection metadata only).
            trusted_proxy_hops: options.trusted_proxy_hops.unwrap_or(0),
        }
    }
}

/// Nest `ThrottlerGuard` analogue: a route-level guard that consults the
/// [`ThrottlerService`] and the `throttle`/`skip_throttle` route metadata.
/// Rejection is [`crate::core::GuardError::TooManyRequests`] (429 + headers
/// via `GuardError::into_response`). Resolve pulls the service from the
/// application registry, so pair this with `ThrottlerModule` or
/// `use_throttler`.
#[derive(Default)]
pub struct ThrottlerGuard {
    service: Option<Arc<ThrottlerService>>,
    trusted_proxy_hops: u16,
}

#[async_trait::async_trait]
impl crate::core::CanActivate for ThrottlerGuard {
    fn resolve(registry: &ProviderRegistry) -> Self {
        Self {
            service: Some(registry.get::<ThrottlerService>()),
            trusted_proxy_hops: 0,
        }
    }

    async fn can_activate(&self, parts: &Parts) -> Result<(), crate::core::GuardError> {
        use crate::core::GuardError;
        let handler = parts
            .extensions
            .get::<crate::core::HandlerKey>()
            .map(|h| h.0)
            .ok_or_else(|| GuardError::forbidden("missing handler key"))?;
        let Some(spec) = throttle_spec_for(Some(handler), None) else {
            return Ok(());
        };
        let service = self.service.clone().ok_or_else(|| {
            GuardError::forbidden(
                "ThrottlerGuard used without ThrottlerModule/use_throttler — no ThrottlerService",
            )
        })?;
        // Guards run at route level — inside the trusted-proxy middleware —
        // so the per-request extension (installed by `use_trusted_proxy_headers`)
        // is the authoritative hop count. The stored 0 only applies when the
        // app declared no proxy topology; before this, the guard ignored the
        // app topology entirely and keyed every request as one client.
        let trusted_proxy_hops = parts
            .extensions
            .get::<crate::client_ip::TrustedProxyHops>()
            .map(|h| h.0)
            .unwrap_or(self.trusted_proxy_hops);
        let ip = crate::client_ip::rate_limit_key_ip(
            &parts.headers,
            &parts.extensions,
            Some(trusted_proxy_hops),
        );
        match service.check(&format!("{handler}:{ip}"), &spec).await {
            ThrottleOutcome::Allowed { .. } => Ok(()),
            ThrottleOutcome::Limited { retry_after_secs } => Err(GuardError::too_many_requests(
                "ThrottlerException",
                retry_after_secs,
            )),
        }
    }
}

impl std::fmt::Debug for ThrottlerGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThrottlerGuard")
            .field("service", &self.service.is_some())
            .field("trusted_proxy_hops", &self.trusted_proxy_hops)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_spec_parse_valid_and_invalid() {
        assert_eq!(
            ThrottleSpec::parse("5/minute"),
            Some(ThrottleSpec {
                limit: 5,
                window_secs: 60
            })
        );
        assert_eq!(
            ThrottleSpec::parse("10/second"),
            Some(ThrottleSpec {
                limit: 10,
                window_secs: 1
            })
        );
        assert_eq!(
            ThrottleSpec::parse("2/hour"),
            Some(ThrottleSpec {
                limit: 2,
                window_secs: 3600
            })
        );
        assert_eq!(ThrottleSpec::parse("5/day"), None);
        assert_eq!(ThrottleSpec::parse("minute"), None);
        assert_eq!(ThrottleSpec::parse("0/minute"), None);
        assert_eq!(ThrottleSpec::parse("x/minute"), None);
    }

    #[test]
    fn throttle_spec_for_route_precedence() {
        let global = Some(ThrottleSpec {
            limit: 100,
            window_secs: 60,
        });
        MetadataRegistry::set("h_decorated", "throttle", "5/minute");
        MetadataRegistry::set("h_skipped", "skip_throttle", "true");

        // Decorated route overrides global.
        assert_eq!(
            throttle_spec_for(Some("h_decorated"), global),
            Some(ThrottleSpec {
                limit: 5,
                window_secs: 60
            })
        );
        // Skip wins over everything.
        assert_eq!(throttle_spec_for(Some("h_skipped"), global), None);
        // Undecorated route falls back to global.
        assert_eq!(throttle_spec_for(Some("h_plain"), global), global);
        // No handler: global applies.
        assert_eq!(throttle_spec_for(None, global), global);
    }

    #[tokio::test]
    async fn in_memory_window_resets_after_expiry() {
        let backend = InMemoryThrottler::new();
        let spec = ThrottleSpec {
            limit: 1,
            window_secs: 1,
        };
        assert!(matches!(
            backend.check("k", &spec).await,
            ThrottleOutcome::Allowed { remaining: 0 }
        ));
        assert!(matches!(
            backend.check("k", &spec).await,
            ThrottleOutcome::Limited { .. }
        ));
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        assert!(matches!(
            backend.check("k", &spec).await,
            ThrottleOutcome::Allowed { remaining: 0 }
        ));
    }

    #[tokio::test]
    async fn in_memory_storm_admits_at_most_limit_without_deadlock() {
        let backend = Arc::new(InMemoryThrottler::new());
        let spec = ThrottleSpec {
            limit: 20,
            window_secs: 60,
        };
        let mut handles = Vec::new();
        for _ in 0..100 {
            let b = backend.clone();
            handles.push(tokio::spawn(async move {
                tokio::time::timeout(std::time::Duration::from_secs(5), b.check("storm", &spec))
                    .await
                    .expect("no deadlock (5s bound)")
            }));
        }
        let mut allowed = 0u64;
        for h in handles {
            if matches!(h.await.expect("join"), ThrottleOutcome::Allowed { .. }) {
                allowed += 1;
            }
        }
        assert_eq!(
            allowed, 20,
            "exactly `limit` requests admitted under contention"
        );
    }

    #[tokio::test]
    async fn in_memory_keys_are_independent() {
        let backend = InMemoryThrottler::new();
        let spec = ThrottleSpec {
            limit: 1,
            window_secs: 60,
        };
        assert!(matches!(
            backend.check("a", &spec).await,
            ThrottleOutcome::Allowed { .. }
        ));
        assert!(matches!(
            backend.check("b", &spec).await,
            ThrottleOutcome::Allowed { .. }
        ));
        assert!(matches!(
            backend.check("a", &spec).await,
            ThrottleOutcome::Limited { .. }
        ));
    }
}
