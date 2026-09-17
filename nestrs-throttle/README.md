# nestrs-throttle

Route throttling for [nestrs](https://crates.io/crates/nestrs) — the Rust analogue of [`@nestjs/throttler`](https://docs.nestjs.com/security/rate-limiting).

crates.io package name is **`nestrs-throttle`** (`nestrs-throttler` is owned by another crate). The Nest-like API is unchanged: `#[throttle]`, `#[skip_throttle]`, `ThrottlerModule`, `ThrottlerGuard`. The umbrella `nestrs` crate re-exports every public symbol when the `throttler` feature is on.

## Install

```toml
[dependencies]
nestrs-throttle = "1.2.0"
```

Or via the umbrella:

```toml
nestrs = { version = "1.2.0", features = ["throttler"] }
# distributed counters:
# nestrs = { version = "1.2.0", features = ["throttler-redis"] }
```

## Surface

- `ThrottleSpec { limit, window_secs }` — parse `"5/minute"` (`second` / `minute` / `hour`)
- `#[throttle(n, "per")]` / `#[skip_throttle]`
- `NestApplication::use_throttler` / `ThrottlerModule::register`
- Backends: in-memory (default) or Redis (`cache-redis` / umbrella `throttler-redis`)
- `ThrottleKeyGenerator` / `ThrottleSkipper` / `ThrottlerBackend`

See the [throttling guide](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/throttling.mdx).

## License

MIT OR Apache-2.0.
