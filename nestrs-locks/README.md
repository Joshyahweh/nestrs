# nestrs-locks

Lease locks for [nestrs](https://crates.io/crates/nestrs) — NestJS `@nestjs/locks` analogue.

Use for **single-runner cron**, leader election–style critical sections, and any work that must not overlap across process replicas. Locks expire after a TTL so a crashed holder cannot block forever.

## Install

```toml
[dependencies]
nestrs-locks = "1.7.1"
```

Or via the umbrella:

```toml
nestrs = { version = "1.7.1", features = ["locks"] }
# Redis backend (optional):
# nestrs-locks = { version = "1.7.1", features = ["cache-redis"] }
```

## Register

```rust
use nestrs_locks::{LocksModule, LocksOptions};

#[module(imports = [LocksModule::register(LocksOptions::default())])]
struct AppModule;
```

## Acquire

```rust
use nestrs_locks::LocksService;
use std::sync::Arc;

async fn nightly(locks: Arc<LocksService>) {
    if let Some(_guard) = locks.try_acquire("cron:nightly").await {
        // only one replica runs this
        do_work().await;
    } // drop guard → release

    // or:
    let _ = locks
        .with_lock("invoice:batch", || async { reconcile().await })
        .await;
}

async fn do_work() {}
async fn reconcile() {}
```

`try_acquire` returns `None` if another holder owns the name. `with_lock` runs the closure only when the lock is obtained.

## Surface

- `LocksModule` / `LocksOptions` / `LocksService`
- `LockGuard` (RAII release)
- `InMemoryLockStore` / `SharedInMemoryLockStore`
- `RedisLockStore` (`cache-redis`)
- `LockStore` trait for custom backends

## Docs

- Guide: [Locks](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/locks.mdx)
- rustdoc: <https://docs.rs/nestrs-locks/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
