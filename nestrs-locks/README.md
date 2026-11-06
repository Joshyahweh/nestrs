# nestrs-locks

NestJS `@nestjs/locks` analogue — lease locks for single-runner cron and critical sections.

```toml
nestrs-locks = "1.7.0"
# or: nestrs = { version = "1.7.0", features = ["locks"] }
```

```rust
use nestrs_locks::{LocksModule, LocksOptions};

LocksModule::register(LocksOptions::default());
// inject LocksService → try_acquire / with_lock
```

## Docs

- Mintlify: `mintlify-docs/ecosystem/locks.mdx`
- API: `mintlify-docs/api/crates/nestrs-locks.mdx`
- rustdoc: <https://docs.rs/nestrs-locks/1.7.0>
