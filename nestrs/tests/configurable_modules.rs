use nestrs::prelude::*;

#[derive(Clone)]
struct MyOptions {
    value: u32,
}

#[injectable]
struct MyService {
    opts: std::sync::Arc<ModuleOptions<MyOptions, MyConfigModule>>,
}

impl MyService {
    fn value(&self) -> u32 {
        self.opts.get().value
    }
}

#[module(
    providers = [MyService],
    exports = [MyService],
)]
struct MyConfigModule;

#[test]
fn configurable_module_builder_for_root_overrides_options() {
    let dm =
        ConfigurableModuleBuilder::<MyOptions>::for_root::<MyConfigModule>(MyOptions { value: 42 });
    let service = dm.registry.get::<MyService>();
    assert_eq!(service.value(), 42);
}

#[tokio::test]
async fn configurable_module_builder_for_root_async_overrides_options() {
    let dm =
        ConfigurableModuleBuilder::<MyOptions>::for_root_async::<MyConfigModule, _, _>(|| async {
            MyOptions { value: 7 }
        })
        .await;
    let service = dm.registry.get::<MyService>();
    assert_eq!(service.value(), 7);
}
