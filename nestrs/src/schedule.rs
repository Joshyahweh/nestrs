use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::module;
use std::sync::Arc;

#[doc(hidden)]
pub use linkme;
#[doc(hidden)]
pub use tokio;
#[doc(hidden)]
pub use tokio_cron_scheduler;

pub type Job = tokio_cron_scheduler::job::JobLocked;

/// A self-contained future for one `#[interval]` task (native tokio timer loop).
///
/// Interval tasks do **not** go through `tokio-cron-scheduler`, which truncates
/// repeat periods to whole seconds (`Duration::as_secs`) and silently stops
/// sub-second jobs after their first tick(s).
pub type IntervalFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

pub struct ScheduleRegistration {
    /// Builds cron-syntax jobs (from `#[cron("...")]` handlers).
    pub build: fn(&ProviderRegistry) -> Vec<Job>,
    /// Builds ready-to-spawn interval loops (from `#[interval(ms)]` handlers).
    pub build_intervals: fn(&ProviderRegistry) -> Vec<IntervalFuture>,
}

#[linkme::distributed_slice]
pub static SCHEDULE_REGISTRATIONS: [ScheduleRegistration] = [..];

/// Wire all `#[schedule_routes]` tasks into a running scheduler.
///
/// Called automatically by `nestrs` during application bootstrap (feature: `schedule`).
pub async fn wire_scheduled_tasks(registry: &ProviderRegistry) {
    if SCHEDULE_REGISTRATIONS.is_empty() {
        return;
    }
    let runtime = registry.get::<ScheduleRuntime>();
    runtime.start(registry).await;
}

pub struct ScheduleRuntime {
    scheduler: tokio::sync::Mutex<Option<tokio_cron_scheduler::JobScheduler>>,
    interval_handles: tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

#[nestrs::async_trait]
impl Injectable for ScheduleRuntime {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            scheduler: tokio::sync::Mutex::new(None),
            interval_handles: tokio::sync::Mutex::new(Vec::new()),
        })
    }

    async fn on_application_shutdown(&self) {
        self.shutdown().await;
    }
}

impl ScheduleRuntime {
    pub async fn start(&self, registry: &ProviderRegistry) {
        // Fast path: already started.
        {
            let guard = self.scheduler.lock().await;
            if guard.is_some() {
                return;
            }
        }

        // Cron-syntax tasks go through tokio-cron-scheduler.
        let mut cron_jobs = Vec::new();
        for reg in SCHEDULE_REGISTRATIONS.iter() {
            cron_jobs.extend((reg.build)(registry));
        }

        if !cron_jobs.is_empty() {
            let sched = tokio_cron_scheduler::JobScheduler::new()
                .await
                .unwrap_or_else(|e| panic!("ScheduleRuntime: failed to create scheduler: {e:?}"));

            for job in cron_jobs {
                let _ = sched
                    .add(job)
                    .await
                    .unwrap_or_else(|e| panic!("ScheduleRuntime: failed to add job: {e:?}"));
            }

            sched
                .start()
                .await
                .unwrap_or_else(|e| panic!("ScheduleRuntime: failed to start scheduler: {e:?}"));

            let mut guard = self.scheduler.lock().await;
            if guard.is_none() {
                *guard = Some(sched);
            }
        }

        // Interval tasks run on native tokio timers (millisecond precision).
        let mut handles = self.interval_handles.lock().await;
        if !handles.is_empty() {
            return;
        }
        for reg in SCHEDULE_REGISTRATIONS.iter() {
            for fut in (reg.build_intervals)(registry) {
                handles.push(tokio::spawn(fut));
            }
        }
    }

    pub async fn shutdown(&self) {
        {
            let mut handles = self.interval_handles.lock().await;
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
        let sched = {
            let mut guard = self.scheduler.lock().await;
            guard.take()
        };
        if let Some(mut sched) = sched {
            let _ = sched.shutdown().await;
        }
    }
}

#[module(providers = [ScheduleRuntime], exports = [ScheduleRuntime])]
pub struct ScheduleModule;

impl ScheduleModule {
    pub fn for_root() -> DynamicModule {
        DynamicModule::from_module::<ScheduleModule>()
    }
}
