use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::module;
use serde::{de::DeserializeOwned, Serialize};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CacheError {
    pub message: String,
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CacheError {}

#[derive(Debug, Clone)]
pub enum CacheOptions {
    InMemory,
    #[cfg(feature = "cache-redis")]
    Redis(RedisCacheOptions),
}

impl CacheOptions {
    pub fn in_memory() -> Self {
        Self::InMemory
    }

    #[cfg(feature = "cache-redis")]
    pub fn redis(url: impl Into<String>) -> Self {
        Self::Redis(RedisCacheOptions::new(url))
    }
}

#[cfg(feature = "cache-redis")]
#[derive(Debug, Clone)]
pub struct RedisCacheOptions {
    pub url: String,
    pub prefix: Option<String>,
}

#[cfg(feature = "cache-redis")]
impl RedisCacheOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            prefix: None,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }
}

#[derive(Clone)]
struct CacheEntry {
    value: serde_json::Value,
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .map(|t| t <= Instant::now())
            .unwrap_or(false)
    }

    fn ttl(&self) -> Option<Duration> {
        self.expires_at.and_then(|t| t.checked_duration_since(Instant::now()))
    }
}

enum CacheBackend {
    InMemory {
        inner: tokio::sync::RwLock<HashMap<String, CacheEntry>>,
    },
    #[cfg(feature = "cache-redis")]
    Redis {
        client: redis::Client,
        prefix: Option<String>,
        conn: tokio::sync::OnceCell<redis::aio::MultiplexedConnection>,
    },
}

impl CacheBackend {
    #[cfg(feature = "cache-redis")]
    fn redis_key(&self, key: &str) -> String {
        let CacheBackend::Redis { prefix, .. } = self else {
            return key.to_string();
        };
        let Some(p) = prefix.as_deref() else {
            return key.to_string();
        };
        let p = p.trim_end_matches(':');
        if p.is_empty() {
            key.to_string()
        } else {
            format!("{p}:{key}")
        }
    }

    #[cfg(feature = "cache-redis")]
    async fn redis_conn(&self) -> Result<redis::aio::MultiplexedConnection, CacheError> {
        let CacheBackend::Redis { client, conn, .. } = self else {
            return Err(CacheError {
                message: "cache backend is not redis".to_string(),
            });
        };
        let c = conn
            .get_or_try_init(|| async {
                client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(|e| CacheError {
                        message: format!("redis connect failed: {e}"),
                    })
            })
            .await?;
        Ok(c.clone())
    }
}

/// Cache service (in-memory by default; Redis available with feature `cache-redis`).
pub struct CacheService {
    backend: CacheBackend,
}

#[nestrs::async_trait]
impl Injectable for CacheService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            backend: CacheBackend::InMemory {
                inner: tokio::sync::RwLock::new(HashMap::new()),
            },
        })
    }
}

impl CacheService {
    fn from_options(options: CacheOptions) -> Result<Self, CacheError> {
        match options {
            CacheOptions::InMemory => Ok(Self {
                backend: CacheBackend::InMemory {
                    inner: tokio::sync::RwLock::new(HashMap::new()),
                },
            }),
            #[cfg(feature = "cache-redis")]
            CacheOptions::Redis(opts) => {
                let client = redis::Client::open(opts.url.clone()).map_err(|e| CacheError {
                    message: format!("redis client open failed: {e}"),
                })?;
                Ok(Self {
                    backend: CacheBackend::Redis {
                        client,
                        prefix: opts.prefix,
                        conn: tokio::sync::OnceCell::new(),
                    },
                })
            }
        }
    }

    pub async fn get_json(&self, key: &str) -> Option<serde_json::Value> {
        match &self.backend {
            CacheBackend::InMemory { inner } => {
                let mut guard = inner.write().await;
                let entry = guard.get(key).cloned()?;
                if entry.is_expired() {
                    guard.remove(key);
                    return None;
                }
                Some(entry.value)
            }
            #[cfg(feature = "cache-redis")]
            CacheBackend::Redis { .. } => {
                let mut conn = self.backend.redis_conn().await.ok()?;
                let rk = self.backend.redis_key(key);
                let raw: Option<String> = redis::cmd("GET")
                    .arg(&rk)
                    .query_async::<Option<String>>(&mut conn)
                    .await
                    .ok()?;
                let raw = raw?;
                serde_json::from_str(&raw).ok()
            }
        }
    }

    pub async fn get<T>(&self, key: &str) -> Result<Option<T>, CacheError>
    where
        T: DeserializeOwned,
    {
        match &self.backend {
            CacheBackend::InMemory { .. } => {
                let Some(value) = self.get_json(key).await else {
                    return Ok(None);
                };
                serde_json::from_value(value).map(Some).map_err(|e| CacheError {
                    message: format!("cache decode failed: {e}"),
                })
            }
            #[cfg(feature = "cache-redis")]
            CacheBackend::Redis { .. } => {
                let mut conn = self.backend.redis_conn().await?;
                let rk = self.backend.redis_key(key);
                let raw: Option<String> = redis::cmd("GET")
                    .arg(&rk)
                    .query_async::<Option<String>>(&mut conn)
                    .await
                    .map_err(|e| CacheError {
                        message: format!("redis get failed: {e}"),
                    })?;
                let Some(raw) = raw else {
                    return Ok(None);
                };
                serde_json::from_str(&raw).map(Some).map_err(|e| CacheError {
                    message: format!("cache decode failed: {e}"),
                })
            }
        }
    }

    pub async fn set_json(&self, key: impl Into<String>, value: serde_json::Value, ttl: Option<Duration>) {
        match &self.backend {
            CacheBackend::InMemory { inner } => {
                let expires_at = ttl.and_then(|d| Instant::now().checked_add(d));
                let mut guard = inner.write().await;
                guard.insert(
                    key.into(),
                    CacheEntry {
                        value,
                        expires_at,
                    },
                );
            }
            #[cfg(feature = "cache-redis")]
            CacheBackend::Redis { .. } => {
                let key = key.into();
                let rk = self.backend.redis_key(&key);
                let mut conn = match self.backend.redis_conn().await {
                    Ok(c) => c,
                    Err(_) => return,
                };
                let payload = match serde_json::to_string(&value) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                let mut cmd = redis::cmd("SET");
                cmd.arg(&rk).arg(payload);
                if let Some(ttl) = ttl {
                    // Use millisecond precision to match in-memory TTL granularity.
                    cmd.arg("PX").arg(ttl.as_millis().min(u128::from(i64::MAX as u64)) as i64);
                }
                let _: redis::RedisResult<()> = cmd.query_async::<()>(&mut conn).await;
            }
        }
    }

    pub async fn set<T>(&self, key: impl Into<String>, value: &T, ttl: Option<Duration>) -> Result<(), CacheError>
    where
        T: Serialize,
    {
        let v = serde_json::to_value(value).map_err(|e| CacheError {
            message: format!("cache encode failed: {e}"),
        })?;
        self.set_json(key, v, ttl).await;
        Ok(())
    }

    pub async fn del(&self, key: &str) -> bool {
        match &self.backend {
            CacheBackend::InMemory { inner } => {
                let mut guard = inner.write().await;
                guard.remove(key).is_some()
            }
            #[cfg(feature = "cache-redis")]
            CacheBackend::Redis { .. } => {
                let rk = self.backend.redis_key(key);
                let mut conn = match self.backend.redis_conn().await {
                    Ok(c) => c,
                    Err(_) => return false,
                };
                let n: i64 = redis::cmd("DEL")
                    .arg(&rk)
                    .query_async::<i64>(&mut conn)
                    .await
                    .unwrap_or(0);
                n > 0
            }
        }
    }

    pub async fn ttl(&self, key: &str) -> Option<Duration> {
        match &self.backend {
            CacheBackend::InMemory { inner } => {
                let mut guard = inner.write().await;
                let entry = guard.get(key).cloned()?;
                if entry.is_expired() {
                    guard.remove(key);
                    return None;
                }
                entry.ttl()
            }
            #[cfg(feature = "cache-redis")]
            CacheBackend::Redis { .. } => {
                let rk = self.backend.redis_key(key);
                let mut conn = self.backend.redis_conn().await.ok()?;
                let ms: i64 = redis::cmd("PTTL")
                    .arg(&rk)
                    .query_async::<i64>(&mut conn)
                    .await
                    .ok()?;
                if ms <= 0 {
                    return None;
                }
                Some(Duration::from_millis(ms as u64))
            }
        }
    }
}

/// In-memory cache module (exporting [`CacheService`]).
#[module(providers = [CacheService], exports = [CacheService])]
pub struct CacheModule;

impl CacheModule {
    pub fn register(options: CacheOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();

        let cache = Arc::new(
            CacheService::from_options(options)
                .unwrap_or_else(|e| panic!("CacheModule::register failed: {e}")),
        );
        registry.override_provider::<CacheService>(cache);

        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![TypeId::of::<CacheService>()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_set_get_del_round_trips() {
        let registry = ProviderRegistry::new();
        let cache = CacheService::construct(&registry);

        cache.set("k", &serde_json::json!({"v": 1}), None)
            .await
            .unwrap();
        let v: serde_json::Value = cache.get("k").await.unwrap().unwrap();
        assert_eq!(v["v"], 1);

        assert!(cache.del("k").await);
        let missing: Option<serde_json::Value> = cache.get("k").await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn cache_ttl_expires_entries() {
        let registry = ProviderRegistry::new();
        let cache = CacheService::construct(&registry);

        cache.set("k", &serde_json::json!({"v": 1}), Some(Duration::from_millis(30)))
            .await
            .unwrap();
        assert!(cache.ttl("k").await.is_some());
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(cache.get_json("k").await.is_none());
        assert!(cache.ttl("k").await.is_none());
    }
}

