use crate::store::{InMemoryIdempotencyStore, SharedIdempotencyStore};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct IdempotencyOptions {
    pub store: SharedIdempotencyStore,
    pub header_name: String,
    pub required: bool,
    /// Optional scope prefix (e.g. user id) applied before the key.
    pub scope: Option<String>,
}

impl Default for IdempotencyOptions {
    fn default() -> Self {
        Self {
            store: Arc::new(InMemoryIdempotencyStore::new(Duration::from_secs(
                24 * 3600,
            ))),
            header_name: "idempotency-key".into(),
            required: false,
            scope: None,
        }
    }
}

impl IdempotencyOptions {
    pub fn in_memory(ttl: Duration) -> Self {
        Self {
            store: Arc::new(InMemoryIdempotencyStore::new(ttl)),
            ..Default::default()
        }
    }
}
