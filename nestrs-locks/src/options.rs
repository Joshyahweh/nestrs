use crate::store::{SharedInMemoryLockStore, SharedLockStore};
use std::time::Duration;

#[derive(Clone)]
pub struct LocksOptions {
    pub store: SharedLockStore,
    pub default_ttl: Duration,
}

impl Default for LocksOptions {
    fn default() -> Self {
        Self {
            store: SharedInMemoryLockStore::new(),
            default_ttl: Duration::from_secs(30),
        }
    }
}
