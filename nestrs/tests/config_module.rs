use nestrs::prelude::*;
use serial_test::serial;
use std::sync::Arc;

#[derive(serde::Deserialize, validator::Validate, nestrs::NestConfig)]
struct AppConfig {
    #[validate(range(min = 1, max = 65535))]
    port: u16,
}

#[injectable]
struct ConfigConsumer {
    config: Arc<AppConfig>,
}

#[module(imports = [ConfigModule::<AppConfig>], providers = [ConfigConsumer])]
struct AppModule;

#[tokio::test]
#[serial]
async fn config_module_exports_typed_config_provider() {
    std::env::set_var("NESTRS_ENV", "production");
    std::env::set_var("PORT", "1234");

    let (registry, _) = <AppModule as Module>::build();
    let consumer = registry.get::<ConfigConsumer>();
    assert_eq!(consumer.config.port, 1234);
}
