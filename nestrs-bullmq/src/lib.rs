//! BullMQ-compatible Redis producer for nestrs.
//!
//! nestrs `queues-redis` uses LPUSH/BRPOP envelopes. Node `@nestjs/bullmq`
//! workers speak the BullMQ key layout. This producer writes jobs that a
//! BullMQ (or bullmq-compatible) worker can pick up from Redis.
//!
//! Key layout (prefix `bull` by default):
//! - `{prefix}:{queue}:id` — INCR job id
//! - `{prefix}:{queue}:{id}` — job hash (`name`, `data`, `opts`, `timestamp`)
//! - `{prefix}:{queue}:wait` — LPUSH job id

#![doc(html_root_url = "https://docs.rs/nestrs-bullmq/1.3.0")]

use redis::AsyncCommands;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Errors from Redis or JSON encoding.
#[derive(Debug, thiserror::Error)]
pub enum BullMqError {
    /// Redis client/connection error.
    #[error(transparent)]
    Redis(#[from] redis::RedisError),
}

/// Producer that enqueues BullMQ-shaped jobs.
#[derive(Clone)]
pub struct BullMqProducer {
    client: redis::Client,
    prefix: String,
    queue: String,
}

impl BullMqProducer {
    /// Connect using a Redis URL (`redis://127.0.0.1/`).
    pub fn new(url: impl AsRef<str>, queue: impl Into<String>) -> Result<Self, BullMqError> {
        Ok(Self {
            client: redis::Client::open(url.as_ref())?,
            prefix: "bull".to_string(),
            queue: queue.into(),
        })
    }

    /// Override the key prefix (BullMQ default is `bull`).
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    fn id_key(&self) -> String {
        format!("{}:{}:id", self.prefix, self.queue)
    }

    fn wait_key(&self) -> String {
        format!("{}:{}:wait", self.prefix, self.queue)
    }

    fn job_key(&self, id: i64) -> String {
        format!("{}:{}:{id}", self.prefix, self.queue)
    }

    /// Add a named job. `data` is stored as a JSON string on the hash (BullMQ).
    pub async fn add(&self, name: &str, data: &Value) -> Result<i64, BullMqError> {
        let mut conn = self.client.get_multiplexed_async_connection().await?;
        let id: i64 = conn.incr(self.id_key(), 1).await?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let data_s = data.to_string();
        let opts = "{}";
        let _: () = redis::pipe()
            .hset(self.job_key(id), "name", name)
            .hset(self.job_key(id), "data", data_s)
            .hset(self.job_key(id), "opts", opts)
            .hset(self.job_key(id), "timestamp", timestamp)
            .lpush(self.wait_key(), id)
            .query_async(&mut conn)
            .await?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::BullMqProducer;

    #[test]
    fn keys_follow_bullmq_layout() {
        let producer = BullMqProducer::new("redis://127.0.0.1/", "email")
            .expect("url parses without a live server");
        assert_eq!(producer.id_key(), "bull:email:id");
        assert_eq!(producer.wait_key(), "bull:email:wait");
        assert_eq!(producer.job_key(7), "bull:email:7");
    }
}
