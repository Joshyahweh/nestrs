#![cfg(feature = "schedule")]

use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::module;
use std::sync::Arc;

#[doc(hidden)]
pub use linkme;
#[doc(hidden)]
pub use tokio_cron_scheduler;

pub type Job = tokio_cron_scheduler::job::JobLocked;

pub struct ScheduleRegistration {
    pub build: fn(&ProviderRegistry) -> Vec<Job>,
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
}

#[nestrs::async_trait]
impl Injectable for ScheduleRuntime {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            scheduler: tokio::sync::Mutex::new(None),
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

        let sched = tokio_cron_scheduler::JobScheduler::new()
            .await
            .unwrap_or_else(|e| panic!("ScheduleRuntime: failed to create scheduler: {e:?}"));

        for reg in SCHEDULE_REGISTRATIONS.iter() {
            let jobs = (reg.build)(registry);
            for job in jobs {
                let _ = sched
                    .add(job)
                    .await
                    .unwrap_or_else(|e| panic!("ScheduleRuntime: failed to add job: {e:?}"));
            }
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

    pub async fn shutdown(&self) {
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

