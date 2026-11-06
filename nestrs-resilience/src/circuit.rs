use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Consecutive-failure circuit breaker with open duration (NestJS resilience analogue).
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_threshold: u32,
    open_for: Duration,
    failures: AtomicU32,
    opened_at_ms: AtomicU64,
    epoch: Instant,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, open_for: Duration) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            open_for,
            failures: AtomicU32::new(0),
            opened_at_ms: AtomicU64::new(0),
            epoch: Instant::now(),
        }
    }

    pub fn state(&self) -> CircuitState {
        let opened = self.opened_at_ms.load(Ordering::Acquire);
        if opened == 0 {
            return CircuitState::Closed;
        }
        let now = self.epoch.elapsed().as_millis() as u64;
        let elapsed = now.saturating_sub(opened);
        if elapsed >= self.open_for.as_millis() as u64 {
            CircuitState::HalfOpen
        } else {
            CircuitState::Open
        }
    }

    pub fn retry_after_secs(&self) -> u64 {
        let opened = self.opened_at_ms.load(Ordering::Acquire);
        if opened == 0 {
            return 0;
        }
        let now = self.epoch.elapsed().as_millis() as u64;
        let elapsed = now.saturating_sub(opened);
        let open_ms = self.open_for.as_millis() as u64;
        open_ms.saturating_sub(elapsed).div_ceil(1000)
    }

    /// `Ok(())` if the call may proceed; `Err(retry_after_secs)` if the circuit is open.
    pub fn before_call(&self) -> Result<(), u64> {
        match self.state() {
            CircuitState::Closed | CircuitState::HalfOpen => Ok(()),
            CircuitState::Open => Err(self.retry_after_secs().max(1)),
        }
    }

    pub fn on_success(&self) {
        self.failures.store(0, Ordering::Release);
        self.opened_at_ms.store(0, Ordering::Release);
    }

    pub fn on_failure(&self) {
        let n = self.failures.fetch_add(1, Ordering::AcqRel) + 1;
        if n >= self.failure_threshold {
            let now = self.epoch.elapsed().as_millis() as u64;
            self.opened_at_ms.store(now.max(1), Ordering::Release);
        }
    }
}
