#![cfg(feature = "queues")]

use nestrs::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static PROCESSED: AtomicUsize = AtomicUsize::new(0);

#[injectable]
#[queue_processor("EMAIL")]
struct EmailProcessor;

#[async_trait]
impl QueueHandler for EmailProcessor {
    async fn process(&self, job: QueueJob) -> Result<(), QueueError> {
        assert_eq!(job.queue.as_str(), "EMAIL");
        assert_eq!(job.name, "send");
        assert_eq!(job.payload["to"], "a@b.com");
        PROCESSED.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[module(
    imports = [QueuesModule::register(&[
        QueueConfig::new("EMAIL").with_concurrency(2)
    ])],
    providers = [EmailProcessor],
)]
struct AppModule;

#[tokio::test]
async fn queued_jobs_are_processed_by_registered_processors() {
    let (registry, _router) = <AppModule as Module>::build();

    registry.eager_init_singletons();
    registry.run_on_module_init().await;
    registry.run_on_application_bootstrap().await;

    nestrs::queues::wire_queue_processors(&registry).await;

    let queues = registry.get::<QueuesService>();
    let q = queues.expect_queue("EMAIL");

    q.add("send", &serde_json::json!({"to": "a@b.com"}))
        .await
        .unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if PROCESSED.load(Ordering::Relaxed) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    assert!(
        PROCESSED.load(Ordering::Relaxed) > 0,
        "expected queued job to be processed"
    );

    registry.run_on_application_shutdown().await;
    registry.run_on_module_destroy().await;
}
