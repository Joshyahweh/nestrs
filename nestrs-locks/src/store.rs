use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

#[async_trait]
pub trait LockStore: Send + Sync {
    async fn try_acquire(&self, name: &str, ttl: Duration) -> Option<LockGuard>;
    async fn release(&self, name: &str, token: &str);
}

pub type SharedLockStore = Arc<dyn LockStore>;

pub struct LockGuard {
    store: SharedLockStore,
    name: String,
    token: String,
    released: bool,
}

impl LockGuard {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn release(mut self) {
        if !self.released {
            self.store.release(&self.name, &self.token).await;
            self.released = true;
        }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        let store = self.store.clone();
        let name = self.name.clone();
        let token = self.token.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                store.release(&name, &token).await;
            });
        }
    }
}

struct Entry {
    token: String,
    until: Instant,
}

pub struct InMemoryLockStore {
    inner: Mutex<HashMap<String, Entry>>,
}

impl InMemoryLockStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryLockStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LockStore for InMemoryLockStore {
    async fn try_acquire(&self, name: &str, ttl: Duration) -> Option<LockGuard> {
        // Cannot construct LockGuard with Arc<Self> from &self without Arc wrapping.
        // Use InMemoryLockStore::shared() factory from options instead.
        let _ = (name, ttl);
        None
    }

    async fn release(&self, name: &str, token: &str) {
        let mut guard = self.inner.lock().await;
        if let Some(e) = guard.get(name) {
            if e.token == token {
                guard.remove(name);
            }
        }
    }
}

/// Arc-backed in-memory store (preferred).
pub struct SharedInMemoryLockStore {
    inner: Arc<Mutex<HashMap<String, Entry>>>,
}

impl SharedInMemoryLockStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl LockStore for SharedInMemoryLockStore {
    async fn try_acquire(&self, name: &str, ttl: Duration) -> Option<LockGuard> {
        let mut guard = self.inner.lock().await;
        let now = Instant::now();
        if let Some(e) = guard.get(name) {
            if e.until > now {
                return None;
            }
        }
        let token = Uuid::new_v4().to_string();
        guard.insert(
            name.to_string(),
            Entry {
                token: token.clone(),
                until: now + ttl,
            },
        );
        let store: SharedLockStore = Arc::new(SharedInMemoryLockStore {
            inner: self.inner.clone(),
        });
        Some(LockGuard {
            store,
            name: name.to_string(),
            token,
            released: false,
        })
    }

    async fn release(&self, name: &str, token: &str) {
        let mut guard = self.inner.lock().await;
        if let Some(e) = guard.get(name) {
            if e.token == token {
                guard.remove(name);
            }
        }
    }
}

#[cfg(feature = "cache-redis")]
pub struct RedisLockStore {
    client: redis::Client,
    prefix: String,
}

#[cfg(feature = "cache-redis")]
impl RedisLockStore {
    pub fn new(url: &str) -> redis::RedisResult<Arc<Self>> {
        Ok(Arc::new(Self {
            client: redis::Client::open(url)?,
            prefix: "nestrs:lock:".into(),
        }))
    }
}

#[cfg(feature = "cache-redis")]
#[async_trait]
impl LockStore for RedisLockStore {
    async fn try_acquire(&self, name: &str, ttl: Duration) -> Option<LockGuard> {
        let mut conn = self.client.get_multiplexed_async_connection().await.ok()?;
        let key = format!("{}{}", self.prefix, name);
        let token = Uuid::new_v4().to_string();
        let set: bool = redis::cmd("SET")
            .arg(&key)
            .arg(&token)
            .arg("NX")
            .arg("PX")
            .arg(ttl.as_millis() as u64)
            .query_async(&mut conn)
            .await
            .ok()?;
        if !set {
            return None;
        }
        let store: SharedLockStore = Arc::new(RedisLockStore {
            client: self.client.clone(),
            prefix: self.prefix.clone(),
        });
        Some(LockGuard {
            store,
            name: name.to_string(),
            token,
            released: false,
        })
    }

    async fn release(&self, name: &str, token: &str) {
        use redis::AsyncCommands;
        if let Ok(mut conn) = self.client.get_multiplexed_async_connection().await {
            let key = format!("{}{}", self.prefix, name);
            let current: Option<String> = conn.get(&key).await.ok().flatten();
            if current.as_deref() == Some(token) {
                let _: Result<(), _> = conn.del(key).await;
            }
        }
    }
}
