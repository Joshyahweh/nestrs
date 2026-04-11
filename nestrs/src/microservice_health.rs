//! Readiness helpers when using [`crate::microservices`] transports.

use crate::{HealthIndicator, HealthStatus};

/// Always reports **up** — use [`RedisBrokerHealth`] / [`NatsBrokerHealth`] when you run real brokers.
#[derive(Debug, Default, Clone)]
pub struct BrokerHealthStub {
    name: &'static str,
}

impl BrokerHealthStub {
    pub fn new(name: &'static str) -> Self {
        Self { name }
    }
}

#[async_trait::async_trait]
impl HealthIndicator for BrokerHealthStub {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn check(&self) -> HealthStatus {
        HealthStatus::Up
    }
}

/// Redis `PING` readiness (enable **`microservices-redis`** on `nestrs`).
#[cfg(all(feature = "microservices", feature = "microservices-redis"))]
#[derive(Debug, Clone)]
pub struct RedisBrokerHealth {
    pub url: String,
}

#[cfg(all(feature = "microservices", feature = "microservices-redis"))]
#[async_trait::async_trait]
impl HealthIndicator for RedisBrokerHealth {
    fn name(&self) -> &'static str {
        "redis"
    }

    async fn check(&self) -> HealthStatus {
        let client = match redis::Client::open(self.url.as_str()) {
            Ok(c) => c,
            Err(e) => return HealthStatus::down(e.to_string()),
        };
        let mut conn = match client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => return HealthStatus::down(e.to_string()),
        };
        match redis::cmd("PING").query_async::<String>(&mut conn).await {
            Ok(s) if s == "PONG" => HealthStatus::Up,
            Ok(other) => HealthStatus::down(format!("unexpected PING reply: {other}")),
            Err(e) => HealthStatus::down(e.to_string()),
        }
    }
}

/// NATS TCP connect readiness (enable **`microservices-nats`** on `nestrs`).
#[cfg(all(feature = "microservices", feature = "microservices-nats"))]
#[derive(Debug, Clone)]
pub struct NatsBrokerHealth {
    pub url: String,
}

#[cfg(all(feature = "microservices", feature = "microservices-nats"))]
#[async_trait::async_trait]
impl HealthIndicator for NatsBrokerHealth {
    fn name(&self) -> &'static str {
        "nats"
    }

    async fn check(&self) -> HealthStatus {
        match async_nats::connect(&self.url).await {
            Ok(_c) => HealthStatus::Up,
            Err(e) => HealthStatus::down(e.to_string()),
        }
    }
}
