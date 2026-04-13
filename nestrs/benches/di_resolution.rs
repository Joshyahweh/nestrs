//! Micro-benchmark: singleton resolution from [`nestrs::core::ProviderRegistry`] after eager init.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use nestrs::async_trait;
use nestrs::core::{Injectable, ProviderRegistry};
use std::sync::Arc;

struct BenchInjectable;

#[async_trait]
impl Injectable for BenchInjectable {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(BenchInjectable)
    }
}

fn bench_provider_registry_get_singleton(c: &mut Criterion) {
    let mut registry = ProviderRegistry::new();
    registry.register::<BenchInjectable>();
    registry.eager_init_singletons();

    c.bench_function("provider_registry_get_cached_singleton", |b| {
        b.iter(|| {
            let v: Arc<BenchInjectable> = registry.get();
            black_box(v);
        })
    });
}

criterion_group!(benches, bench_provider_registry_get_singleton);
criterion_main!(benches);
