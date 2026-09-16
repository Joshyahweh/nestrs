//! Drizzle-related error type. Wraps upstream driver errors plus a few
//! nestrs-only variants (`NotConfigured`, `InvalidUrl`).

use std::fmt;

/// Errors that can originate from `nestrs-drizzle` operations.
#[derive(Debug)]
pub enum DrizzleError {
    /// `DrizzleModule::for_root` was not called before a service operation.
    NotConfigured,
    /// The connection URL couldn't be parsed.
    InvalidUrl(String),
    /// Driver-level error from drizzle-orm / sqlx / native-tls / etc.
    Driver(String),
    /// Operation timed out before completing.
    Timeout,
}

impl fmt::Display for DrizzleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DrizzleError::NotConfigured => write!(
                f,
                "DrizzleModule::for_root must be called before using DrizzleService"
            ),
            DrizzleError::InvalidUrl(s) => write!(f, "invalid drizzle url: {s}"),
            DrizzleError::Driver(s) => write!(f, "drizzle driver error: {s}"),
            DrizzleError::Timeout => write!(f, "drizzle operation timed out"),
        }
    }
}

impl std::error::Error for DrizzleError {}

pub type Result<T> = std::result::Result<T, DrizzleError>;