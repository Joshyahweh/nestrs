use crate::policy::{BulkheadPolicy, RetryPolicy, TimeoutPolicy};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ResilienceOptions {
    pub defaults: PolicyDefaults,
    pub circuit_failure_threshold: u32,
    pub circuit_open_for: Duration,
}

#[derive(Debug, Clone)]
pub struct PolicyDefaults {
    pub retry: RetryPolicy,
    pub timeout: TimeoutPolicy,
    pub bulkhead: BulkheadPolicy,
}

impl Default for ResilienceOptions {
    fn default() -> Self {
        Self {
            defaults: PolicyDefaults {
                retry: RetryPolicy::default(),
                timeout: TimeoutPolicy::default(),
                bulkhead: BulkheadPolicy::default(),
            },
            circuit_failure_threshold: 5,
            circuit_open_for: Duration::from_secs(30),
        }
    }
}
