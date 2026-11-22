use crate::store::{InMemoryIdempotencyStore, SharedIdempotencyStore};
use std::sync::Arc;
use std::time::Duration;

/// Largest request body buffered when `Idempotency-Key` is present.
///
/// A larger body is rejected with **413** and the handler does not run.
/// Without a cap, the middleware reads the body to the end to fingerprint
/// it, so a client can exhaust memory by attaching the header to an
/// unbounded upload.
pub const DEFAULT_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Largest accepted `Idempotency-Key` value, in bytes.
pub const MAX_IDEMPOTENCY_KEY_BYTES: usize = 4096;

#[derive(Clone)]
pub struct IdempotencyOptions {
    pub store: SharedIdempotencyStore,
    pub header_name: String,
    pub required: bool,
    /// Optional scope prefix (e.g. user id) applied before the key.
    pub scope: Option<String>,
    /// See [`DEFAULT_MAX_BODY_BYTES`].
    pub max_body_bytes: usize,
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
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
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
