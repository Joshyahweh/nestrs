#![cfg(feature = "schedule")]

use nestrs::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static HITS: AtomicUsize = AtomicUsize::new(0);

#[injectable]
struct TasksService;

#[schedule_routes]
impl TasksService {
    #[interval(600)]
    async fn tick(&self) {
        HITS.fetch_add(1, Ordering::Relaxed);
    }
}

#[module(imports = [ScheduleModule::for_root()], providers = [TasksService])]
struct AppModule;

#[tokio::test]
async fn interval_tasks_run_after_wiring_scheduler() {
    let (registry, _router) = <AppModule as Module>::build();

    registry.eager_init_singletons();
    registry.run_on_module_init().await;
    registry.run_on_application_bootstrap().await;

    nestrs::schedule::wire_scheduled_tasks(&registry).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if HITS.load(Ordering::Relaxed) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        HITS.load(Ordering::Relaxed) > 0,
        "expected scheduled task to run at least once"
    );

    registry.run_on_application_shutdown().await;
    registry.run_on_module_destroy().await;
}

