//! Throttle counter backends. [`ThrottlerBackend`] is the trait, with three
//! concrete variants:
//!
//! - [`InMemoryThrottler`] — default, 32-shard, poison-tolerant.
//! - [`RedisThrottler`] — cross-process counters in Redis (cfg `cache-redis`).
//! - [`ThrottlerBackendKind::Custom`] — user-supplied `Arc<dyn ThrottlerBackend>`.

use crate::spec::{ThrottleOutcome, ThrottleSpec};
use std::collections::HashMap;
use std::sync::Arc;

/// Storage backend for throttle counters (Nest `ThrottlerStorage` analogue).
#[async_trait::async_trait]
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

#[async_trait::async_trait]
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
#[async_trait::async_trait]
impl ThrottlerBackend for RedisThrottler {
    async fn check(&self, key: &str, spec: &ThrottleSpec) -> ThrottleOutcome {
        let full_key = format!("{}:{}", self.key_prefix, key);
        let Ok(conn) = self.client.get_multiplexed_tokio_connection().await else {
            // Fail open on backend unavailability (same policy as the
            // global Redis rate limiter): throttling is an optimization,
            // not an availability gate.
            tracing::warn!(target: "nestrs_throttler", "redis throttler: connection failed; allowing request");
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
                tracing::warn!(target: "nestrs_throttler", "redis throttler check failed: {e}");
                ThrottleOutcome::Allowed { remaining: 0 }
            }
        }
    }
}

/// Backend selection for [`crate::ThrottlerOptions`].
#[derive(Debug, Clone, Default)]
pub enum ThrottlerBackendKind {
    #[default]
    InMemory,
    /// Cross-process counters in Redis (requires the `cache-redis` feature).
    #[cfg(feature = "cache-redis")]
    Redis { url: String, key_prefix: String },
    /// User-supplied backend (e.g. DynamoDB, Memcached, CockroachDB). Anything
    /// that implements `ThrottlerBackend` and is cheap to share behind an Arc.
    Custom(Arc<dyn ThrottlerBackend>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
