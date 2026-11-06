//! NestJS [`@nestjs/locks`](https://docs.nestjs.com/techniques/locks) analogue —
//! lease-based distributed locks for single-runner cron and critical sections.
//!
//! # Example
//!
//! ```ignore
//! use nestrs_locks::{LocksModule, LocksOptions, LocksService};
//!
//! let dm = LocksModule::register(LocksOptions::default());
//! # async fn demo(locks: std::sync::Arc<LocksService>) {
//! if let Some(guard) = locks.try_acquire("cron:nightly").await {
//!     // exclusive work
//!     guard.release().await;
//! }
//! # }
//! ```
//!
//! Choose `default_ttl` carefully: shorter than tolerable duplicate work after a crash,
//! longer than healthy job duration (or extend the lease). Redis store: feature `cache-redis`.
//!
//! Umbrella feature: `nestrs` / `locks` → `nestrs::locks`.

#![doc(html_root_url = "https://docs.rs/nestrs-locks/1.7.0")]

mod module;
mod options;
mod store;

pub use module::LocksModule;
pub use options::LocksOptions;
pub use store::{
    InMemoryLockStore, LockGuard, LockStore, SharedInMemoryLockStore, SharedLockStore,
};

#[cfg(feature = "cache-redis")]
pub use store::RedisLockStore;

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
use std::sync::Arc;
use std::time::Duration;

/// High-level lock service (NestJS LocksService analogue).
pub struct LocksService {
    store: SharedLockStore,
    default_ttl: Duration,
}

impl LocksService {
    pub fn from_options(opts: &LocksOptions) -> Self {
        Self {
            store: opts.store.clone(),
            default_ttl: opts.default_ttl,
        }
    }

    pub async fn try_acquire(&self, name: &str) -> Option<LockGuard> {
        self.store.try_acquire(name, self.default_ttl).await
    }

    pub async fn with_lock<F, Fut, T>(&self, name: &str, f: F) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let guard = self.try_acquire(name).await?;
        let out = f().await;
        guard.release().await;
        Some(out)
    }
}

impl Injectable for LocksService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("LocksService must be provided by LocksModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exclusive_lock() {
        let svc = LocksService::from_options(&LocksOptions::default());
        let g1 = svc.try_acquire("cron").await.expect("first");
        assert!(svc.try_acquire("cron").await.is_none());
        g1.release().await;
        assert!(svc.try_acquire("cron").await.is_some());
    }
}
