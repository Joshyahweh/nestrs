use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// Typed resilience failure for HTTP/RPC mapping.
#[derive(Debug, Clone)]
pub enum ResilienceError {
    Timeout,
    CircuitOpen { retry_after_secs: u64 },
    BulkheadRejected,
    ExhaustedRetries { attempts: u32, message: String },
    Fallback { message: String },
    Other(String),
}

impl std::fmt::Display for ResilienceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "operation timed out"),
            Self::CircuitOpen { retry_after_secs } => {
                write!(f, "circuit open; retry after {retry_after_secs}s")
            }
            Self::BulkheadRejected => write!(f, "bulkhead rejected"),
            Self::ExhaustedRetries { attempts, message } => {
                write!(f, "retries exhausted after {attempts}: {message}")
            }
            Self::Fallback { message } => write!(f, "fallback: {message}"),
            Self::Other(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ResilienceError {}

/// Events emitted by policies (NestJS ResilienceEvents analogue).
#[derive(Debug, Clone)]
pub enum ResilienceEvent {
    Retry {
        label: String,
        attempt: u32,
    },
    Timeout {
        label: String,
    },
    CircuitOpen {
        label: String,
    },
    CircuitHalfOpen {
        label: String,
    },
    CircuitClosed {
        label: String,
    },
    CircuitRejected {
        label: String,
    },
    BulkheadRejected {
        label: String,
    },
    Fallback {
        label: String,
    },
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub attempts: u32,
    pub delay: Duration,
    pub factor: f64,
    pub max_delay: Duration,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            delay: Duration::from_secs(1),
            factor: 2.0,
            max_delay: Duration::from_secs(30),
            jitter: true,
        }
    }
}

impl RetryPolicy {
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let mut ms = self.delay.as_millis() as f64 * self.factor.powi(attempt as i32);
        let max = self.max_delay.as_millis() as f64;
        if ms > max {
            ms = max;
        }
        if self.jitter {
            let j = rand::random::<f64>() * 0.2;
            ms *= 1.0 + j;
        }
        Duration::from_millis(ms as u64)
    }
}

#[derive(Debug, Clone)]
pub struct TimeoutPolicy {
    pub timeout: Duration,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BulkheadPolicy {
    pub max_concurrent: usize,
}

impl Default for BulkheadPolicy {
    fn default() -> Self {
        Self {
            max_concurrent: 64,
        }
    }
}

pub type FallbackFn<T> = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<T, ResilienceError>> + Send>> + Send + Sync,
>;

#[derive(Clone, Default)]
pub struct PolicyBundle {
    pub retry: Option<RetryPolicy>,
    pub timeout: Option<TimeoutPolicy>,
    pub bulkhead: Option<BulkheadPolicy>,
    pub circuit_name: Option<String>,
}
