//! Criterion microbench — resilience policy overhead (Wave 5 perf gate).

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nestrs_resilience::{
    PolicyBundle, ResilienceError, ResilienceOptions, ResilienceService, RetryPolicy, TimeoutPolicy,
};
use std::sync::Arc;
use std::time::Duration;

fn bench_execute_ok(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let svc = Arc::new(ResilienceService::from_options(ResilienceOptions::default()));
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
        ..Default::default()
    };

    c.bench_function("resilience_execute_ok", |b| {
        b.to_async(&rt).iter(|| {
            let svc = Arc::clone(&svc);
            let policy = policy.clone();
            async move {
                let out = svc
                    .execute("bench", policy, || async { Ok::<_, ResilienceError>(1u32) })
                    .await
                    .expect("execute");
                black_box(out)
            }
        });
    });
}

criterion_group!(benches, bench_execute_ok);
criterion_main!(benches);
