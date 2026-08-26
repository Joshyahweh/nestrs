//! MongoDB integration (NestJS **MongooseModule** analogue). Features: **`mongo`** (TLS + BSON compat),
//! optional **`mongo-dns`** for **`mongodb+srv://`** / SRV seed lists (adds the driver's **`dns-resolver`** stack).

use crate::core::DatabasePing;
use crate::{injectable, module};
use async_trait::async_trait;
use mongodb::Client;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

static MONGO_URI: OnceLock<String> = OnceLock::new();
static MONGO_CLIENT: OnceCell<Client> = OnceCell::const_new();

async fn ensure_client() -> Result<&'static Client, String> {
    MONGO_CLIENT
        .get_or_try_init(|| async {
            let uri = MONGO_URI.get().cloned().ok_or_else(|| {
                "MongoModule::for_root must be called before using MongoService".to_string()
            })?;
            Client::with_uri_str(&uri)
                .await
                .map_err(|e| format!("mongodb connect: {e}"))
        })
        .await
}

/// Injectable MongoDB service (feature: **`mongo`**).
///
/// Call [`MongoModule::for_root`] once before `NestFactory::create` to set the
/// connection URI, and import the module via `DynamicModule::from_module`
/// (`for_root` returns `Self`, not a `DynamicModule`, so it can't be used inline):
///
/// ```ignore
/// #[tokio::main]
/// async fn main() {
///     nestrs::MongoModule::for_root("mongodb://127.0.0.1:27017");
///
///     NestFactory::create::<AppModule>().listen(3000).await;
/// }
///
/// #[module(imports = [DynamicModule::from_module::<nestrs::MongoModule>()])]
/// pub struct AppModule;
/// ```
#[injectable]
pub struct MongoService;

impl MongoService {
    pub async fn client(&self) -> Result<Client, String> {
        ensure_client().await.cloned()
    }

    pub async fn database(&self, name: &str) -> Result<mongodb::Database, String> {
        Ok(self.client().await?.database(name))
    }

    pub async fn ping(&self) -> Result<(), String> {
        let c = ensure_client().await?;
        c.list_database_names()
            .await
            .map_err(|e| format!("mongodb ping: {e}"))?;
        Ok(())
    }
}

#[async_trait]
impl DatabasePing for MongoService {
    async fn ping_database(&self) -> Result<(), String> {
        self.ping().await
    }
}

#[module(providers = [MongoService], exports = [MongoService])]
pub struct MongoModule;

impl MongoModule {
    pub fn for_root(uri: impl Into<String>) -> Self {
        let _ = MONGO_URI.set(uri.into());
        Self
    }
}
