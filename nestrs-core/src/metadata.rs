//! Lightweight metadata registry for custom decorator patterns.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

type Registry = HashMap<String, HashMap<String, String>>;

fn store() -> &'static RwLock<Registry> {
    static STORE: OnceLock<RwLock<Registry>> = OnceLock::new();
    STORE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Global metadata helpers (handler key + metadata key/value).
pub struct MetadataRegistry;

impl MetadataRegistry {
    pub fn set(handler: impl Into<String>, key: impl Into<String>, value: impl Into<String>) {
        let mut guard = store().write().expect("metadata lock poisoned");
        let entry = guard.entry(handler.into()).or_default();
        entry.insert(key.into(), value.into());
    }

    pub fn get(handler: &str, key: &str) -> Option<String> {
        let guard = store().read().expect("metadata lock poisoned");
        guard.get(handler).and_then(|m| m.get(key)).cloned()
    }

    /// Removes all handler metadata entries in this process.
    ///
    /// **Available only with the `test-hooks` feature.** For tests; see `STABILITY.md` in the repo root.
    #[cfg(feature = "test-hooks")]
    pub fn clear_for_tests() {
        let mut guard = store().write().expect("metadata lock poisoned");
        guard.clear();
    }
}
