use nestrs::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static MODULE_INIT: AtomicBool = AtomicBool::new(false);
static APP_BOOTSTRAP: AtomicBool = AtomicBool::new(false);
static APP_SHUTDOWN: AtomicBool = AtomicBool::new(false);
static MODULE_DESTROY: AtomicBool = AtomicBool::new(false);

struct HookedService;

#[async_trait]
impl Injectable for HookedService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self)
    }

    async fn on_module_init(&self) {
        MODULE_INIT.store(true, Ordering::SeqCst);
    }

    async fn on_application_bootstrap(&self) {
        APP_BOOTSTRAP.store(true, Ordering::SeqCst);
    }

    async fn on_application_shutdown(&self) {
        APP_SHUTDOWN.store(true, Ordering::SeqCst);
    }

    async fn on_module_destroy(&self) {
        MODULE_DESTROY.store(true, Ordering::SeqCst);
    }
}

#[module(providers = [HookedService])]
struct AppModule;

#[tokio::test]
async fn lifecycle_hooks_are_callable_from_registry() {
    let (registry, _) = <AppModule as Module>::build();

    registry.eager_init_singletons();
    registry.run_on_module_init().await;
    registry.run_on_application_bootstrap().await;

    assert!(MODULE_INIT.load(Ordering::SeqCst));
    assert!(APP_BOOTSTRAP.load(Ordering::SeqCst));

    registry.run_on_application_shutdown().await;
    registry.run_on_module_destroy().await;

    assert!(APP_SHUTDOWN.load(Ordering::SeqCst));
    assert!(MODULE_DESTROY.load(Ordering::SeqCst));
}
