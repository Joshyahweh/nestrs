//! Generic database connectivity hook (NestJS “Database” techniques chapter analogue).

use async_trait::async_trait;

/// Minimal health / connectivity check shared by SQL, Prisma, Mongo, and custom drivers.
#[async_trait]
pub trait DatabasePing: Send + Sync {
    /// Cheap round-trip (for example `SELECT 1` or `ping` command).
    async fn ping_database(&self) -> Result<(), String>;
}
