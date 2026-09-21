# nestrs-health

Terminus-style health checks for nestrs — liveness / readiness / startup
decorators, indicator trait, and standard indicators (database, HTTP
dependency, disk space, broker).

Extracted from `nestrs/src/health_probes.rs` and
`nestrs/src/microservice_health.rs` so apps that only need health checks can
depend on just this crate. The umbrella `nestrs` crate re-exports every
public symbol at the same path, so existing code keeps compiling unchanged.

## Install

```toml
[dependencies]
nestrs-health = "1.4.0"
```

## Surface

| Symbol | Feature | Purpose |
|--------|---------|---------|
| `HealthIndicator` (trait) | — | async trait `name()` + `check() -> HealthStatus` |
| `HealthStatus` (enum) | — | `Up` / `Down { message }` |
| `ReadinessContext` | — | indicator-list holder (umbrella uses this) |
| `DatabaseIndicator` | — | readiness over `nestrs_core::DatabasePing` |
| `HttpIndicator` | `http` | GET a dependency URL with timeout |
| `DiskSpaceIndicator` | `disk` | `statvfs` free-space threshold (unix) |
| `BrokerHealthStub` | — | always-up placeholder |
| `RedisBrokerHealth` | `redis` | `PING`/`PONG` round-trip |
| `NatsBrokerHealth` | `nats` | TCP connect |
| `ProbeKind` (enum) | — | Liveness / Readiness / Startup |
| `ProbeOutcome` (enum) | — | Up / Down for a probe run |
| `install_probes` | — | mount the three fixed probe endpoints |

## Why a separate crate?

- Lets apps pull just health-check primitives without the rest of the
  framework (and its compile-time cost).
- Keeps the umbrella's `Cargo.toml` slim: optional deps like `libc`,
  `reqwest`, `redis`, and `async-nats` move behind `nestrs-health`'s own
  feature flags.
- Same path-preserving shim pattern as `nestrs-security` and
  `nestrs-throttle`: nothing in `nestrs::health_probes::*`,
  `nestrs::microservice_health::*`, or the `nestrs::HealthIndicator` /
  `HealthStatus` / `ProbeKind` names moves at the user level.
