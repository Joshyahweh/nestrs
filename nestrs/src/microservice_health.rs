//! Broker readiness indicators for the `nestrs-microservices` transports.
//!
//! Thin re-export shim over the `nestrs-health` workspace member. Real
//! implementation lives in `nestrs-health::microservice_indicators`; the
//! umbrella crate keeps the same path so existing code
//! (`nestrs::microservice_health::BrokerHealthStub`,
//! `RedisBrokerHealth`, `NatsBrokerHealth`) keeps compiling unchanged.
//! Feature gates on the underlying crate mirror the umbrella's
//! `microservices-redis` / `microservices-nats` flags.

pub use nestrs_health::microservice_indicators::*;
