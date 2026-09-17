//! Redis-backed [`tower_sessions::SessionStore`] (feature **`session-redis`**).
//!
//! Production counterpart of [`tower_sessions::MemoryStore`]. Keys are
//! `{prefix}:{session_id}` (default prefix `nestrs:session`). The session id
//! is a URL-safe bearer credential — it is used as the Redis key (required)
//! but never written to logs or `Debug` output. Connection URLs redact
//! `user:pass@` the same way [`crate::mongo`] and the Redis throttler do.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use redis::AsyncCommands;
use time::OffsetDateTime;
use tower_sessions::session::{Id, Record};
use tower_sessions::session_store::{self, SessionStore};

struct RedisSessionInner {
    client: redis::Client,
    url_redacted: String,
    conn: tokio::sync::OnceCell<redis::aio::MultiplexedConnection>,
}

/// Redis [`SessionStore`] using `SET` / `GET` / `DEL` with `PX` TTL.
///
/// Construct with [`RedisSessionStore::new`] (or
/// [`NestApplication::use_session_redis`](crate::NestApplication::use_session_redis)
/// which installs the layer for you). `Clone` shares the Redis client
/// (required by axum's `Layer` bound on `SessionManagerLayer`).
#[derive(Clone)]
pub struct RedisSessionStore {
    inner: Arc<RedisSessionInner>,
    prefix: String,
}

impl fmt::Debug for RedisSessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisSessionStore")
            .field("prefix", &self.prefix)
            .field("url", &self.inner.url_redacted)
            .finish_non_exhaustive()
    }
}

/// Redact `user:pass@` from a Redis URL so `Debug` / errors cannot leak
/// credentials. URLs without userinfo are returned unchanged.
fn redact_userinfo(uri: &str) -> String {
    let Some(scheme_end) = uri.find("://") else {
        return uri.to_string();
    };
    let rest = &uri[scheme_end + 3..];
    let Some(at) = rest.find('@') else {
        return uri.to_string();
    };
    format!("{}***@{}", &uri[..=scheme_end + 2], &rest[at + 1..])
}

impl RedisSessionStore {
    /// Open a Redis client from `url`. The URL is stored only in redacted
    /// form for `Debug`.
    pub fn new(url: impl AsRef<str>) -> Result<Self, String> {
        let url = url.as_ref();
        let client = redis::Client::open(url).map_err(|_| {
            format!(
                "nestrs: invalid Redis session URL ({})",
                redact_userinfo(url)
            )
        })?;
        Ok(Self {
            inner: Arc::new(RedisSessionInner {
                client,
                url_redacted: redact_userinfo(url),
                conn: tokio::sync::OnceCell::new(),
            }),
            prefix: "nestrs:session".to_string(),
        })
    }

    /// Override the key prefix (default `nestrs:session`).
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    fn key(&self, id: &Id) -> String {
        format!("{}:{id}", self.prefix)
    }

    async fn connection(&self) -> session_store::Result<redis::aio::MultiplexedConnection> {
        self.inner
            .conn
            .get_or_try_init(|| async {
                self.inner
                    .client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(|e| {
                        session_store::Error::Backend(format!("redis session connect failed: {e}"))
                    })
            })
            .await
            .cloned()
    }
}

fn ttl_millis(expiry: OffsetDateTime) -> i64 {
    let ms = (expiry - OffsetDateTime::now_utc()).whole_milliseconds();
    if ms <= 0 {
        0
    } else if ms > i64::MAX as i128 {
        i64::MAX
    } else {
        ms as i64
    }
}

#[async_trait]
impl SessionStore for RedisSessionStore {
    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let payload =
            serde_json::to_vec(record).map_err(|e| session_store::Error::Encode(e.to_string()))?;
        let mut conn = self.connection().await?;
        let key = self.key(&record.id);
        let ttl = ttl_millis(record.expiry_date);
        if ttl <= 0 {
            let _: () = conn.del(&key).await.map_err(|e| {
                session_store::Error::Backend(format!("redis session delete failed: {e}"))
            })?;
            return Ok(());
        }
        redis::cmd("SET")
            .arg(&key)
            .arg(&payload)
            .arg("PX")
            .arg(ttl)
            .query_async::<()>(&mut conn)
            .await
            .map_err(|e| {
                session_store::Error::Backend(format!("redis session save failed: {e}"))
            })?;
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let mut conn = self.connection().await?;
        let key = self.key(session_id);
        let payload: Option<Vec<u8>> = conn.get(&key).await.map_err(|e| {
            session_store::Error::Backend(format!("redis session load failed: {e}"))
        })?;
        let Some(bytes) = payload else {
            return Ok(None);
        };
        let record: Record = serde_json::from_slice(&bytes)
            .map_err(|e| session_store::Error::Decode(e.to_string()))?;
        if record.expiry_date <= OffsetDateTime::now_utc() {
            let _: () = conn.del(&key).await.map_err(|e| {
                session_store::Error::Backend(format!("redis session delete failed: {e}"))
            })?;
            return Ok(None);
        }
        Ok(Some(record))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        let mut conn = self.connection().await?;
        let key = self.key(session_id);
        let _: () = conn.del(&key).await.map_err(|e| {
            session_store::Error::Backend(format!("redis session delete failed: {e}"))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_url_userinfo() {
        let store =
            RedisSessionStore::new("redis://ada:s3cret@127.0.0.1:6379/0").expect("url parses");
        let dbg = format!("{store:?}");
        assert!(
            !dbg.contains("s3cret"),
            "password must not appear in Debug: {dbg}"
        );
        assert!(
            dbg.contains("***@127.0.0.1:6379/0"),
            "userinfo should be redacted, got: {dbg}"
        );
    }

    #[test]
    fn new_rejects_empty_url_without_leaking() {
        let err = RedisSessionStore::new("").expect_err("empty url");
        assert!(err.contains("invalid Redis session URL"));
        assert!(!err.contains("s3cret"));
    }
}
