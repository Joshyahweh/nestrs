use nestrs::prelude::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

mod direct_export_visibility {
    use super::*;

    #[derive(Default)]
    #[injectable]
    pub struct DbService;

    pub struct ConsumerService {
        db: Arc<DbService>,
    }

    impl ConsumerService {
        pub fn ready(&self) -> bool {
            Arc::strong_count(&self.db) >= 1
        }
    }

    impl Injectable for ConsumerService {
        fn construct(registry: &ProviderRegistry) -> Arc<Self> {
            Arc::new(Self {
                db: registry.get::<DbService>(),
            })
        }
    }

    #[module(
        providers = [DbService],
        exports = [DbService],
    )]
    pub struct DataModule;

    #[module(
        imports = [DataModule],
        providers = [ConsumerService],
    )]
    pub struct AppModule;

    #[test]
    fn exported_provider_is_visible_to_importer() {
        let (registry, _) = <AppModule as Module>::build();
        assert!(registry.get::<ConsumerService>().ready());
    }
}

mod re_export_visibility {
    use super::*;

    #[derive(Default)]
    #[injectable]
    pub struct DbService;

    pub struct ConsumerService {
        db: Arc<DbService>,
    }

    impl ConsumerService {
        pub fn ready(&self) -> bool {
            Arc::strong_count(&self.db) >= 1
        }
    }

    impl Injectable for ConsumerService {
        fn construct(registry: &ProviderRegistry) -> Arc<Self> {
            Arc::new(Self {
                db: registry.get::<DbService>(),
            })
        }
    }

    #[module(
        providers = [DbService],
        exports = [DbService],
    )]
    pub struct DataModule;

    #[module(
        imports = [DataModule],
        re_exports = [DataModule],
    )]
    pub struct BridgeModule;

    #[module(
        imports = [BridgeModule],
        providers = [ConsumerService],
    )]
    pub struct AppModule;

    #[test]
    fn re_exported_provider_is_visible_transitively() {
        let (registry, _) = <AppModule as Module>::build();
        assert!(registry.get::<ConsumerService>().ready());
    }
}

mod private_provider_visibility {
    use super::*;

    #[derive(Default)]
    #[injectable]
    pub struct DbService;

    pub struct ConsumerService {
        _db: Arc<DbService>,
    }

    impl Injectable for ConsumerService {
        fn construct(registry: &ProviderRegistry) -> Arc<Self> {
            Arc::new(Self {
                _db: registry.get::<DbService>(),
            })
        }
    }

    #[module(
        providers = [DbService],
    )]
    pub struct PrivateModule;

    #[module(
        imports = [PrivateModule],
        providers = [ConsumerService],
    )]
    pub struct AppModule;

    #[test]
    fn non_exported_provider_is_not_visible() {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let (registry, _) = <AppModule as Module>::build();
            let _ = registry.get::<ConsumerService>();
        }));
        assert!(result.is_err());
    }
}
