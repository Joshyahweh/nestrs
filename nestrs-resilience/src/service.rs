use crate::circuit::CircuitBreaker;
use crate::options::ResilienceOptions;
use crate::policy::{
    PolicyBundle, ResilienceError, ResilienceEvent, RetryPolicy, TimeoutPolicy,
};
use nestrs_core::Injectable;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Semaphore};

/// Executes async work under resilience policies (NestJS ResilienceService analogue).
pub struct ResilienceService {
    options: ResilienceOptions,
    circuits: Mutex<HashMap<String, Arc<CircuitBreaker>>>,
    bulkhead: Arc<Semaphore>,
    events_tx: broadcast::Sender<ResilienceEvent>,
}

impl ResilienceService {
    pub fn from_options(options: ResilienceOptions) -> Self {
        let max = options.defaults.bulkhead.max_concurrent.max(1);
        let (events_tx, _) = broadcast::channel(256);
        Self {
            options,
            circuits: Mutex::new(HashMap::new()),
            bulkhead: Arc::new(Semaphore::new(max)),
            events_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ResilienceEvent> {
        self.events_tx.subscribe()
    }

    fn emit(&self, event: ResilienceEvent) {
        let _ = self.events_tx.send(event);
    }

    fn circuit(&self, name: &str) -> Arc<CircuitBreaker> {
        let mut guard = self.circuits.lock().expect("circuits lock");
        guard
            .entry(name.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(
                    self.options.circuit_failure_threshold,
                    self.options.circuit_open_for,
                ))
            })
            .clone()
    }

    /// Run `op` under the given policy bundle.
    pub async fn execute<T, F, Fut>(
        &self,
        label: impl Into<String>,
        policy: PolicyBundle,
        op: F,
    ) -> Result<T, ResilienceError>
    where
        T: Send + 'static,
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, ResilienceError>> + Send,
    {
        let label = label.into();
        let retry = policy
            .retry
            .unwrap_or_else(|| self.options.defaults.retry.clone());
        let timeout = policy
            .timeout
            .unwrap_or_else(|| self.options.defaults.timeout.clone());

        if policy.bulkhead.is_some() {
            let permit = self
                .bulkhead
                .clone()
                .try_acquire_owned()
                .map_err(|_| {
                    self.emit(ResilienceEvent::BulkheadRejected {
                        label: label.clone(),
                    });
                    ResilienceError::BulkheadRejected
                })?;
            let result = self
                .execute_inner(&label, &retry, &timeout, policy.circuit_name.as_deref(), &op)
                .await;
            drop(permit);
            return result;
        }

        self.execute_inner(&label, &retry, &timeout, policy.circuit_name.as_deref(), &op)
            .await
    }

    async fn execute_inner<T, F, Fut>(
        &self,
        label: &str,
        retry: &RetryPolicy,
        timeout: &TimeoutPolicy,
        circuit_name: Option<&str>,
        op: &F,
    ) -> Result<T, ResilienceError>
    where
        T: Send + 'static,
        F: Fn() -> Fut + Send + Sync,
        Fut: Future<Output = Result<T, ResilienceError>> + Send,
    {
        let circuit = circuit_name.map(|n| self.circuit(n));
        let mut last_err = ResilienceError::Other("no attempts".into());
        let attempts = retry.attempts.max(1);

        for attempt in 0..attempts {
            if let Some(ref c) = circuit {
                if let Err(retry_after) = c.before_call() {
                    self.emit(ResilienceEvent::CircuitRejected {
                        label: label.to_string(),
                    });
                    return Err(ResilienceError::CircuitOpen {
                        retry_after_secs: retry_after,
                    });
                }
            }

            if attempt > 0 {
                self.emit(ResilienceEvent::Retry {
                    label: label.to_string(),
                    attempt,
                });
                tokio::time::sleep(retry.backoff_for_attempt(attempt - 1)).await;
            }

            let fut = op();
            let timed = tokio::time::timeout(timeout.timeout, fut);
            match timed.await {
                Ok(Ok(v)) => {
                    if let Some(ref c) = circuit {
                        let was_half = matches!(c.state(), crate::circuit::CircuitState::HalfOpen);
                        c.on_success();
                        if was_half {
                            self.emit(ResilienceEvent::CircuitClosed {
                                label: label.to_string(),
                            });
                        }
                    }
                    return Ok(v);
                }
                Ok(Err(e)) => {
                    if let Some(ref c) = circuit {
                        c.on_failure();
                        if matches!(c.state(), crate::circuit::CircuitState::Open) {
                            self.emit(ResilienceEvent::CircuitOpen {
                                label: label.to_string(),
                            });
                        }
                    }
                    last_err = e;
                }
                Err(_) => {
                    self.emit(ResilienceEvent::Timeout {
                        label: label.to_string(),
                    });
                    if let Some(ref c) = circuit {
                        c.on_failure();
                    }
                    last_err = ResilienceError::Timeout;
                }
            }
        }

        Err(ResilienceError::ExhaustedRetries {
            attempts,
            message: last_err.to_string(),
        })
    }

    /// Convenience: timeout-only execute.
    pub async fn with_timeout<T, Fut>(
        &self,
        label: impl Into<String>,
        timeout: Duration,
        fut: Fut,
    ) -> Result<T, ResilienceError>
    where
        T: Send + 'static,
        Fut: Future<Output = Result<T, ResilienceError>> + Send,
    {
        let label = label.into();
        match tokio::time::timeout(timeout, fut).await {
            Ok(r) => r,
            Err(_) => {
                self.emit(ResilienceEvent::Timeout { label });
                Err(ResilienceError::Timeout)
            }
        }
    }
}

impl Injectable for ResilienceService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("ResilienceService must be provided by ResilienceModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyBundle;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn retries_until_success() {
        let svc = ResilienceService::from_options(ResilienceOptions::default());
        let n = AtomicU32::new(0);
        let out = svc
            .execute(
                "t",
                PolicyBundle {
                    retry: Some(RetryPolicy {
                        attempts: 3,
                        delay: Duration::from_millis(1),
                        factor: 1.0,
                        max_delay: Duration::from_millis(1),
                        jitter: false,
                    }),
                    timeout: Some(TimeoutPolicy {
                        timeout: Duration::from_secs(1),
                    }),
                    ..Default::default()
                },
                || async {
                    let v = n.fetch_add(1, Ordering::SeqCst);
                    if v < 2 {
                        Err(ResilienceError::Other("fail".into()))
                    } else {
                        Ok(42u32)
                    }
                },
            )
            .await
            .expect("ok");
        assert_eq!(out, 42);
        assert_eq!(n.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn circuit_opens_after_failures() {
        let mut opts = ResilienceOptions::default();
        opts.circuit_failure_threshold = 2;
        opts.circuit_open_for = Duration::from_secs(60);
        let svc = ResilienceService::from_options(opts);
        let policy = PolicyBundle {
            retry: Some(RetryPolicy {
                attempts: 1,
                delay: Duration::from_millis(1),
                factor: 1.0,
                max_delay: Duration::from_millis(1),
                jitter: false,
            }),
            timeout: Some(TimeoutPolicy {
                timeout: Duration::from_secs(1),
            }),
            circuit_name: Some("db".into()),
            ..Default::default()
        };
        let _ = svc
            .execute("a", policy.clone(), || async {
                Err::<(), _>(ResilienceError::Other("x".into()))
            })
            .await;
        let _ = svc
            .execute("a", policy.clone(), || async {
                Err::<(), _>(ResilienceError::Other("x".into()))
            })
            .await;
        let err = svc
            .execute("a", policy, || async { Ok(()) })
            .await
            .expect_err("open");
        assert!(matches!(err, ResilienceError::CircuitOpen { .. }));
    }
}
