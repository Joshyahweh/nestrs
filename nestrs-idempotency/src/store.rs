use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    /// Returns `Some(record)` if the key was already completed.
    /// Returns `None` if this caller should execute the handler (first writer).
    /// Returns `Err` if another request is in-flight for the same key.
    async fn begin_or_get(
        &self,
        scope_key: &str,
    ) -> Result<Option<IdempotencyRecord>, IdempotencyBusy>;

    async fn complete(&self, scope_key: &str, record: IdempotencyRecord);
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
    InFlight { since: Instant },
    Done { record: IdempotencyRecord, at: Instant },
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
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn begin_or_get(
        &self,
        scope_key: &str,
    ) -> Result<Option<IdempotencyRecord>, IdempotencyBusy> {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        guard.retain(|_, slot| match slot {
            Slot::InFlight { since } => now.duration_since(*since) < self.ttl,
            Slot::Done { at, .. } => now.duration_since(*at) < self.ttl,
        });
        match guard.get(scope_key) {
            Some(Slot::Done { record, .. }) => Ok(Some(record.clone())),
            Some(Slot::InFlight { .. }) => Err(IdempotencyBusy),
            None => {
                guard.insert(
                    scope_key.to_string(),
                    Slot::InFlight { since: now },
                );
                Ok(None)
            }
        }
    }

    async fn complete(&self, scope_key: &str, record: IdempotencyRecord) {
        let mut guard = self.inner.lock().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn second_inflight_caller_is_busy() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        assert!(store.begin_or_get("a:k1").await.unwrap().is_none());
        assert!(store.begin_or_get("a:k1").await.is_err());
    }

    #[tokio::test]
    async fn completed_key_replays_record() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        assert!(store.begin_or_get("a:k1").await.unwrap().is_none());
        store
            .complete(
                "a:k1",
                IdempotencyRecord {
                    status: 201,
                    body: b"{\"ok\":true}".to_vec(),
                    content_type: "application/json".into(),
                },
            )
            .await;
        let replay = store.begin_or_get("a:k1").await.unwrap().expect("replay");
        assert_eq!(replay.status, 201);
        assert_eq!(replay.body, b"{\"ok\":true}");
    }

    #[tokio::test]
    async fn concurrent_begin_only_one_winner() {
        let store = Arc::new(InMemoryIdempotencyStore::new(Duration::from_secs(60)));
        let mut handles = Vec::new();
        for _ in 0..32 {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(async move { s.begin_or_get("race:key").await }));
        }
        let mut winners = 0usize;
        let mut busy = 0usize;
        for h in handles {
            match h.await.expect("join") {
                Ok(None) => winners += 1,
                Ok(Some(_)) => panic!("unexpected completed record before complete()"),
                Err(_) => busy += 1,
            }
        }
        assert_eq!(winners, 1, "exactly one first-writer");
        assert_eq!(busy, 31);
    }
}

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
    async fn begin_or_get(
        &self,
        scope_key: &str,
    ) -> Result<Option<IdempotencyRecord>, IdempotencyBusy> {
        use redis::AsyncCommands;
        let mut conn = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|_| IdempotencyBusy)?;
        let key = format!("{}{}", self.prefix, scope_key);
        let lock_key = format!("{key}:lock");
        let done: Option<String> = conn.get(&key).await.unwrap_or(None);
        if let Some(raw) = done {
            if let Ok(rec) = serde_json::from_str(&raw) {
                return Ok(Some(rec));
            }
        }
        let set: bool = redis::cmd("SET")
            .arg(&lock_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(self.ttl_secs)
            .query_async(&mut conn)
            .await
            .unwrap_or(false);
        if !set {
            return Err(IdempotencyBusy);
        }
        Ok(None)
    }

    async fn complete(&self, scope_key: &str, record: IdempotencyRecord) {
        use redis::AsyncCommands;
        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let key = format!("{}{}", self.prefix, scope_key);
            let lock_key = format!("{key}:lock");
            if let Ok(raw) = serde_json::to_string(&record) {
                let _: Result<(), _> = conn.set_ex(&key, raw, self.ttl_secs).await;
            }
            let _: Result<(), _> = conn.del(lock_key).await;
        }
    }
}
