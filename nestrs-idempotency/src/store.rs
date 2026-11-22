use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Result of [`IdempotencyStore::begin_or_get`].
#[derive(Debug)]
pub enum IdempotencyClaim {
    /// This caller holds the in-flight lock. Pass `token` to
    /// [`IdempotencyStore::complete`]. A later `complete` with a different
    /// token must not publish a body or delete the lock.
    Acquired { token: String },
    /// The key already has a stored response.
    Completed(IdempotencyRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
    /// SHA-256 fingerprint of method, target, body, and caller credentials.
    ///
    /// Empty on records written before fingerprinting. The middleware refuses
    /// to replay those: a missing fingerprint is not proof the new request
    /// matches the one that produced the body.
    #[serde(default)]
    pub fingerprint: String,
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Returns [`IdempotencyClaim::Completed`] if the key was already completed.
    /// Returns [`IdempotencyClaim::Acquired`] if this caller should execute the
    /// handler (first writer). The `token` is a fence: `complete` publishes
    /// only while that token still owns the lock.
    /// Returns `Err` if another request is in-flight for the same key.
    async fn begin_or_get(&self, scope_key: &str) -> Result<IdempotencyClaim, IdempotencyBusy>;

    /// Store `record` if `token` still owns the in-flight lock.
    ///
    /// A stale owner (the lock expired and another caller acquired it, or a
    /// response was already published) must not overwrite the body or delete
    /// the new lock. That was a second execution window: `DEL` of a lock this
    /// caller no longer held let a third request run the handler.
    async fn complete(&self, scope_key: &str, token: &str, record: IdempotencyRecord);
}

/// Unique per acquisition. The value is a fence, not a secret: `complete` is
/// not an HTTP entry point. The address distinguishes processes; the counter
/// distinguishes acquisitions inside one process.
fn new_fence_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let process = std::ptr::from_ref(&COUNTER) as usize;
    format!("{process:x}-{nanos:x}-{n:x}-{}", std::process::id())
}

#[derive(Debug, Clone)]
pub struct IdempotencyBusy;

impl std::fmt::Display for IdempotencyBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "idempotency key in flight")
    }
}

impl std::error::Error for IdempotencyBusy {}

#[derive(Debug)]
enum Slot {
    InFlight {
        since: Instant,
        token: String,
    },
    Done {
        record: IdempotencyRecord,
        at: Instant,
    },
}

pub struct InMemoryIdempotencyStore {
    inner: Mutex<HashMap<String, Slot>>,
    ttl: Duration,
}

impl InMemoryIdempotencyStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Test double for "the lock expired and another caller acquired it".
    #[cfg(test)]
    async fn testing_replace_inflight_token(&self, scope_key: &str, token: &str) {
        let mut guard = self.inner.lock().await;
        if let Some(Slot::InFlight { token: held, .. }) = guard.get_mut(scope_key) {
            *held = token.to_string();
        }
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn begin_or_get(&self, scope_key: &str) -> Result<IdempotencyClaim, IdempotencyBusy> {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        guard.retain(|_, slot| match slot {
            Slot::InFlight { since, .. } => now.duration_since(*since) < self.ttl,
            Slot::Done { at, .. } => now.duration_since(*at) < self.ttl,
        });
        match guard.get(scope_key) {
            Some(Slot::Done { record, .. }) => Ok(IdempotencyClaim::Completed(record.clone())),
            Some(Slot::InFlight { .. }) => Err(IdempotencyBusy),
            None => {
                let token = new_fence_token();
                guard.insert(
                    scope_key.to_string(),
                    Slot::InFlight {
                        since: now,
                        token: token.clone(),
                    },
                );
                Ok(IdempotencyClaim::Acquired { token })
            }
        }
    }

    async fn complete(&self, scope_key: &str, token: &str, record: IdempotencyRecord) {
        let mut guard = self.inner.lock().await;
        match guard.get(scope_key) {
            // Another acquisition owns the lock. Publishing or clearing the
            // slot would let a third caller run the handler.
            Some(Slot::InFlight { token: held, .. }) if held != token => return,
            // First publisher wins. A late owner must not replace the body.
            Some(Slot::Done { .. }) => return,
            Some(Slot::InFlight { .. }) | None => {}
        }
        guard.insert(
            scope_key.to_string(),
            Slot::Done {
                record,
                at: Instant::now(),
            },
        );
    }
}

pub type SharedIdempotencyStore = Arc<dyn IdempotencyStore>;

#[cfg(feature = "cache-redis")]
pub struct RedisIdempotencyStore {
    client: redis::Client,
    ttl_secs: u64,
    prefix: String,
}

#[cfg(feature = "cache-redis")]
impl RedisIdempotencyStore {
    pub fn new(url: &str, ttl: Duration) -> redis::RedisResult<Self> {
        Ok(Self {
            client: redis::Client::open(url)?,
            ttl_secs: ttl.as_secs().max(1),
            prefix: "nestrs:idem:".into(),
        })
    }
}

#[cfg(feature = "cache-redis")]
#[async_trait]
impl IdempotencyStore for RedisIdempotencyStore {
    async fn begin_or_get(&self, scope_key: &str) -> Result<IdempotencyClaim, IdempotencyBusy> {
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| IdempotencyBusy)?;
        let key = format!("{}{}", self.prefix, scope_key);
        let lock_key = format!("{key}:lock");
        let token = new_fence_token();
        // GET then SET NX is not atomic: a peer can finish, store the
        // response, and delete the lock between the two commands, and this
        // caller would run the handler again. Claim-or-read in one script.
        // The lock value is the fence token, not a constant, so a stale
        // owner cannot delete a lock it no longer holds.
        let reply: redis::RedisResult<(i64, String)> = redis::cmd("EVAL")
            .arg(
                r#"local existing = redis.call("GET", KEYS[1])
if existing then
  return {1, existing}
end
if redis.call("SET", KEYS[2], ARGV[2], "NX", "EX", ARGV[1]) then
  return {0, ""}
end
return {2, ""}"#,
            )
            .arg(2)
            .arg(&key)
            .arg(&lock_key)
            .arg(self.ttl_secs)
            .arg(&token)
            .query_async(&mut conn)
            .await;
        let (status, payload) = reply.map_err(|_| IdempotencyBusy)?;
        match status {
            0 => Ok(IdempotencyClaim::Acquired { token }),
            1 => serde_json::from_str(&payload)
                .map(IdempotencyClaim::Completed)
                .map_err(|_| IdempotencyBusy),
            _ => Err(IdempotencyBusy),
        }
    }

    async fn complete(&self, scope_key: &str, token: &str, record: IdempotencyRecord) {
        let Ok(mut conn) = self.client.get_multiplexed_async_connection().await else {
            return;
        };
        let key = format!("{}{}", self.prefix, scope_key);
        let lock_key = format!("{key}:lock");
        let Ok(raw) = serde_json::to_string(&record) else {
            return;
        };
        // Publish and drop the lock in one script, and only if this token
        // still owns it. An unconditional DEL removes a lock another caller
        // acquired after this one expired, and that caller then runs the
        // handler too. If the lock is already gone, SET NX stores the body
        // only when nobody else published first.
        let _: redis::RedisResult<i64> = redis::cmd("EVAL")
            .arg(
                r#"local lock = redis.call("GET", KEYS[2])
if lock == false then
  if redis.call("SET", KEYS[1], ARGV[2], "NX", "EX", ARGV[3]) then
    return 1
  end
  return 0
end
if lock ~= ARGV[1] then
  return 0
end
redis.call("SET", KEYS[1], ARGV[2], "EX", ARGV[3])
redis.call("DEL", KEYS[2])
return 1"#,
            )
            .arg(2)
            .arg(&key)
            .arg(&lock_key)
            .arg(token)
            .arg(raw)
            .arg(self.ttl_secs)
            .query_async(&mut conn)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn record(body: &str) -> IdempotencyRecord {
        IdempotencyRecord {
            status: 201,
            body: body.as_bytes().to_vec(),
            content_type: "application/json".into(),
            fingerprint: "fp".into(),
        }
    }

    #[tokio::test]
    async fn second_inflight_caller_is_busy() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        assert!(matches!(
            store.begin_or_get("a:k1").await.unwrap(),
            IdempotencyClaim::Acquired { .. }
        ));
        assert!(store.begin_or_get("a:k1").await.is_err());
    }

    #[tokio::test]
    async fn completed_key_replays_record() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        let IdempotencyClaim::Acquired { token } = store.begin_or_get("a:k1").await.unwrap() else {
            panic!("first writer");
        };
        store
            .complete("a:k1", &token, record("{\"ok\":true}"))
            .await;
        let IdempotencyClaim::Completed(replay) = store.begin_or_get("a:k1").await.unwrap() else {
            panic!("replay");
        };
        assert_eq!(replay.status, 201);
        assert_eq!(replay.body, b"{\"ok\":true}");
    }

    #[tokio::test]
    async fn wrong_token_does_not_publish_or_clear_the_lock() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        let IdempotencyClaim::Acquired { token } = store.begin_or_get("a:k1").await.unwrap() else {
            panic!("first writer");
        };
        store
            .complete("a:k1", "not-the-owner", record("stolen"))
            .await;
        assert!(
            store.begin_or_get("a:k1").await.is_err(),
            "a foreign token must leave the in-flight lock held"
        );
        store.complete("a:k1", &token, record("owner")).await;
        let IdempotencyClaim::Completed(replay) = store.begin_or_get("a:k1").await.unwrap() else {
            panic!("replay");
        };
        assert_eq!(replay.body, b"owner");
    }

    #[tokio::test]
    async fn stale_owner_does_not_replace_a_new_holders_lock() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        let IdempotencyClaim::Acquired { token: stale } = store.begin_or_get("a:k1").await.unwrap()
        else {
            panic!("first writer");
        };
        store
            .testing_replace_inflight_token("a:k1", "new-holder")
            .await;
        store.complete("a:k1", &stale, record("stale-body")).await;
        assert!(
            store.begin_or_get("a:k1").await.is_err(),
            "the new holder must still be in flight"
        );
        store
            .complete("a:k1", "new-holder", record("new-body"))
            .await;
        let IdempotencyClaim::Completed(replay) = store.begin_or_get("a:k1").await.unwrap() else {
            panic!("replay");
        };
        assert_eq!(replay.body, b"new-body");
    }

    #[tokio::test]
    async fn concurrent_begin_only_one_winner() {
        let store = Arc::new(InMemoryIdempotencyStore::new(Duration::from_secs(60)));
        let mut handles = Vec::new();
        for _ in 0..32 {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(
                async move { s.begin_or_get("race:key").await },
            ));
        }
        let mut winners = 0usize;
        let mut busy = 0usize;
        for h in handles {
            match h.await.expect("join") {
                Ok(IdempotencyClaim::Acquired { .. }) => winners += 1,
                Ok(IdempotencyClaim::Completed(_)) => {
                    panic!("unexpected completed record before complete()")
                }
                Err(_) => busy += 1,
            }
        }
        assert_eq!(winners, 1, "exactly one first-writer");
        assert_eq!(busy, 31);
    }
}
