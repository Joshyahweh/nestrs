//! Route throttling (`#[throttle(n, "per")]` / `#[skip_throttle]`) — NestJS
//! [`@nestjs/throttler`](https://docs.nestjs.com/security/rate-limiting) parity.
//!
//! This module is a thin shim: every public symbol lives in the
//! [`nestrs-throttler`](https://crates.io/crates/nestrs-throttler) crate and
//! is re-exported here when the `throttler` feature flag is on. See
//! `nestrs_throttler` for the implementation (backends, key generators,
//! skippers, guard, middleware, module wiring, etc.).

#[cfg(feature = "throttler")]
pub use nestrs_throttler::*;
