use crate::options::OutboxOptions;
use crate::store::SharedOutboxStore;
use nestrs_core::Injectable;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type PublishFn = Arc<
    dyn Fn(String, serde_json::Value) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

/// Relay pending outbox messages through a user-provided publisher.
pub struct OutboxRelay {
    store: SharedOutboxStore,
    max_attempts: u32,
    batch_size: usize,
}

impl OutboxRelay {
    pub fn from_options(opts: &OutboxOptions) -> Self {
        Self {
            store: opts.store.clone(),
            max_attempts: opts.max_attempts,
            batch_size: opts.batch_size,
        }
    }

    pub fn store(&self) -> SharedOutboxStore {
        self.store.clone()
    }

    /// Enqueue a message (call inside the same logical transaction as business writes).
    pub async fn add(&self, topic: &str, payload: serde_json::Value) -> crate::OutboxMessage {
        self.store.add(topic, payload).await
    }

    /// Drain one batch through `publish`.
    pub async fn tick(&self, publish: &PublishFn) {
        let batch = self.store.claim_batch(self.batch_size).await;
        for msg in batch {
            match publish(msg.topic.clone(), msg.payload.clone()).await {
                Ok(()) => self.store.mark_published(&msg.id).await,
                Err(reason) => {
                    if msg.attempts >= self.max_attempts {
                        self.store.dead_letter(&msg.id, &reason).await;
                    } else {
                        self.store.mark_retry(&msg.id).await;
                    }
                }
            }
        }
    }
}

impl Injectable for OutboxRelay {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("OutboxRelay must be provided by OutboxModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::OutboxOptions;
    use crate::store::OutboxStatus;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn relay_publishes_and_marks() {
        let opts = OutboxOptions::default();
        let relay = OutboxRelay::from_options(&opts);
        relay.add("orders", serde_json::json!({"id": 1})).await;
        let published = Arc::new(AtomicU32::new(0));
        let published_c = published.clone();
        let publish: PublishFn = Arc::new(move |_topic, _payload| {
            published_c.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        });
        relay.tick(&publish).await;
        assert_eq!(published.load(Ordering::SeqCst), 1);
        let pending = opts.store.claim_batch(10).await;
        assert!(pending.iter().all(|m| m.status != OutboxStatus::Pending) || pending.is_empty());
    }
}
