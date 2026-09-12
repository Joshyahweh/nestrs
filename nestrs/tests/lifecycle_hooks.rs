use nestrs::core::ProviderLifecycle;
use nestrs::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

static INIT_ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

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

// --- useValue/useFactory lifecycle (audit #42) ------------------------------
//
// NestJS fires OnModuleInit & friends on any provider object implementing the
// interfaces, including useValue/useFactory providers. The plain
// register_use_value / register_use_factory accept any `T: Send + Sync +
// 'static` (no hook impl to call), so hooks opt in via the ProviderLifecycle
// trait and the *_with_lifecycle registration variants.

type Log = Arc<Mutex<Vec<String>>>;

/// `Tagged<'V'>` / `Tagged<'F'>` are distinct provider types (one per TypeId)
/// sharing a single hook impl; the const char doubles as the event prefix.
struct Tagged<const TAG: char> {
    log: Log,
}

impl<const TAG: char> Tagged<TAG> {
    fn record(&self, event: &str) {
        self.log.lock().unwrap().push(format!("{TAG}:{event}"));
    }
}

#[async_trait]
impl<const TAG: char> ProviderLifecycle for Tagged<TAG> {
    async fn on_module_init(&self) {
        self.record("init");
    }
    async fn on_module_destroy(&self) {
        self.record("destroy");
    }
    async fn on_application_bootstrap(&self) {
        self.record("bootstrap");
    }
    async fn on_before_application_shutdown(&self) {
        self.record("before_shutdown");
    }
    async fn on_application_shutdown(&self) {
        self.record("shutdown");
    }
}

fn assert_log(log: &Log, expected: &[&str]) {
    let got = log.lock().unwrap();
    let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(*got, expected, "hook firing order");
}

#[tokio::test]
async fn use_value_and_factory_lifecycle_hooks_run_through_boot_sequence() {
    let log: Log = Arc::default();
    let mut registry = ProviderRegistry::new();
    registry.register_use_value_with_lifecycle(Arc::new(Tagged::<'V'> { log: log.clone() }));
    registry.register_use_factory_with_lifecycle(ProviderScope::Singleton, {
        let log = log.clone();
        move |_r| Arc::new(Tagged::<'F'> { log: log.clone() })
    });

    // The exact `listen()` boot sequence: eager construction, module init,
    // application bootstrap.
    registry.eager_init_singletons();
    registry.run_on_module_init().await;
    registry.run_on_application_bootstrap().await;
    assert_log(&log, &["V:init", "F:init", "V:bootstrap", "F:bootstrap"]);

    // The exact graceful-shutdown sequence: beforeApplicationShutdown,
    // onApplicationShutdown, onModuleDestroy — ALL reversed (NestJS: every
    // shutdown-side hook runs in reverse init order so dependencies tear
    // down after dependents).
    registry.run_on_before_application_shutdown().await;
    registry.run_on_application_shutdown().await;
    registry.run_on_module_destroy().await;
    assert_log(
        &log,
        &[
            "V:init",
            "F:init",
            "V:bootstrap",
            "F:bootstrap",
            "F:before_shutdown",
            "V:before_shutdown",
            "F:shutdown",
            "V:shutdown",
            "F:destroy",
            "V:destroy",
        ],
    );
}

// --- dependency-ordered hooks (follow-up to audit #43) ----------------------
//
// `ordered_singletons` consumed the recorded `constructor -> dependency`
// edges in the WRONG direction, topologically sorting DEPENDENTS before
// their dependencies — in effect collapsing hook order to registration
// order for dependency-connected providers. A service's `on_module_init`
// could run before the provider it depends on had initialized, and
// destroy hooks tore dependencies down before their dependents. The sort
// now honors the documented "dependencies initialize before dependents"
// contract.

struct ChainLeaf;

#[async_trait]
impl Injectable for ChainLeaf {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self)
    }
    async fn on_module_init(&self) {
        INIT_ORDER.lock().unwrap().push("A:init");
    }
    async fn on_module_destroy(&self) {
        INIT_ORDER.lock().unwrap().push("A:destroy");
    }
}

struct ChainMid;

#[async_trait]
impl Injectable for ChainMid {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        let _ = registry.get::<ChainLeaf>();
        Arc::new(Self)
    }
    async fn on_module_init(&self) {
        INIT_ORDER.lock().unwrap().push("B:init");
    }
    async fn on_module_destroy(&self) {
        INIT_ORDER.lock().unwrap().push("B:destroy");
    }
}

struct ChainTop;

#[async_trait]
impl Injectable for ChainTop {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        let _ = registry.get::<ChainMid>();
        Arc::new(Self)
    }
    async fn on_module_init(&self) {
        INIT_ORDER.lock().unwrap().push("C:init");
    }
    async fn on_module_destroy(&self) {
        INIT_ORDER.lock().unwrap().push("C:destroy");
    }
}

#[tokio::test]
async fn hooks_initialize_dependencies_before_dependents_and_reverse_on_destroy() {
    // Register TOP-FIRST: registration order alone would init C, B, A. The
    // recorded construction edges (C -> B -> A) must override it so the
    // leaf initializes first.
    let mut registry = ProviderRegistry::new();
    registry.register::<ChainTop>();
    registry.register::<ChainMid>();
    registry.register::<ChainLeaf>();

    registry.eager_init_singletons();
    registry.run_on_module_init().await;
    assert_eq!(
        *INIT_ORDER.lock().unwrap(),
        vec!["A:init", "B:init", "C:init"],
        "dependencies must initialize before their dependents"
    );

    // Destroy hooks run in reverse init order: dependents tear down
    // BEFORE the dependencies they still hold references to.
    registry.run_on_module_destroy().await;
    assert_eq!(
        *INIT_ORDER.lock().unwrap(),
        vec![
            "A:init",
            "B:init",
            "C:init",
            "C:destroy",
            "B:destroy",
            "A:destroy"
        ],
        "dependents must destroy before their dependencies"
    );
}
