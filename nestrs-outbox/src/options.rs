use crate::store::{InMemoryOutboxStore, SharedOutboxStore};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct OutboxOptions {
    pub store: SharedOutboxStore,
    pub max_attempts: u32,
    pub poll_interval: Duration,
    pub batch_size: usize,
    pub allow_in_memory: bool,
}

impl Default for OutboxOptions {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryOutboxStore::new()),
            max_attempts: 10,
            poll_interval: Duration::from_secs(1),
            batch_size: 32,
            allow_in_memory: true,
        }
    }
}
