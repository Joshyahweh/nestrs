//! Health probe (Terminus-style) building blocks for nestrs.
//!
//! Extracted from the umbrella `nestrs/src/health_probes.rs` and
//! `nestrs/src/microservice_health.rs` so apps that only need health checks
//! (no full framework) can depend on just this crate. The umbrella `nestrs`
//! crate re-exports every public symbol at the same path, so existing code
//! that writes `nestrs::ProbeKind`, `nestrs::DatabaseIndicator`, or
//! `nestrs::install_probes` keeps compiling unchanged.
//!
//! ## Probe decorators
//!
//! - [`ProbeKind`] — the three fixed probe kinds (Liveness / Readiness / Startup).
//! - [`ProbeOutcome`] — `Up` / `Down { status, message }` for a probe run.
//! - [`install_probes`] — mount the three probe endpoints at the server root
//!   (unaffected by `set_global_prefix` / URI versioning).
//!
//! ## Indicators
//!
//! - [`HealthIndicator`] — async trait implemented by every standard check.
//! - [`HealthStatus`] — `Up` / `Down { message }` result of a single check.
//! - [`ReadinessContext`] — owns the `enable_readiness_check` indicator list;
//!   used by `NestApplication` and the legacy `/__nestrs/health/ready` shape.
//! - [`DatabaseIndicator`] — readiness over the shared
//!   [`nestrs_core::DatabasePing`] capability (SQLx / Prisma / Mongo).
//!
//! ### Behind feature flags
//!
//! - `http` → [`HttpIndicator`] — `GET url` with `timeout`, requires 2xx.
//! - `disk` → [`DiskSpaceIndicator`] — `statvfs` free-space threshold (unix).
//!
//! ## Broker indicators (microservices)
//!
//! - [`BrokerHealthStub`] — always up (default).
//! - `redis` → [`RedisBrokerHealth`] — `PING`/`PONG` round-trip.
//! - `nats` → [`NatsBrokerHealth`] — TCP connect.
//!
//! **Docs:** mdBook **Health** (`docs/src/health.md`).

#![doc(html_root_url = "https://docs.rs/nestrs-health/1.4.0")]

mod indicators;
mod microservice_indicators;
mod probes;

pub use indicators::{DatabaseIndicator, HealthIndicator, HealthStatus, ReadinessContext};

#[cfg(feature = "http")]
pub use indicators::HttpIndicator;

#[cfg(feature = "disk")]
pub use indicators::DiskSpaceIndicator;

pub use microservice_indicators::BrokerHealthStub;

#[cfg(feature = "redis")]
pub use microservice_indicators::RedisBrokerHealth;

#[cfg(feature = "nats")]
pub use microservice_indicators::NatsBrokerHealth;

pub use probes::{install_probes, ProbeKind, ProbeOutcome};
