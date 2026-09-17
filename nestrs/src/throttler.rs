//! Route throttling (`#[throttle(n, "per")]` / `#[skip_throttle]`) — NestJS
//! [`@nestjs/throttler`](https://docs.nestjs.com/security/rate-limiting) parity.
//!
//! This module is a thin shim: every public symbol lives in the
//! [`nestrs-throttle`](https://crates.io/crates/nestrs-throttle) crate and
//! is re-exported here when the `throttler` feature flag is on. See
//! `nestrs_throttle` for the implementation (backends, key generators,
//! skippers, guard, middleware, module wiring, etc.).

#[cfg(feature = "throttler")]
pub use nestrs_throttle::*;
