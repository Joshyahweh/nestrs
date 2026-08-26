//! Lab 5 — cache: live Redis backend (localhost:6380) with key prefixing, TTL
//! expiry, delete, and hit/miss behavior exposed over HTTP.
//!
//! Run: `cargo run -p lab --bin lab5_cache`

use nestrs::prelude::*;
use std::sync::Arc;
use std::time::Duration;

#[derive(serde::Deserialize)]
pub struct KeyParams {
    key: String,
}

#[derive(serde::Deserialize)]
pub struct SetQuery {
    /// Optional TTL in milliseconds.
    ttl_ms: Option<u64>,
}

#[controller(prefix = "/cache")]
pub struct CacheController;

#[routes(state = CacheService)]
impl CacheController {
    #[post("/set/:key")]
    pub async fn set_key(
        State(cache): State<Arc<CacheService>>,
        #[param::param] p: KeyParams,
        #[param::query] q: SetQuery,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        let ttl = q.ttl_ms.map(Duration::from_millis);
        cache.set(&p.key, &body, ttl).await.expect("cache set");
        Json(serde_json::json!({ "stored": p.key, "ttl_ms": q.ttl_ms }))
    }

    #[get("/get/:key")]
    pub async fn get_key(
        State(cache): State<Arc<CacheService>>,
        #[param::param] p: KeyParams,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        match cache.get::<serde_json::Value>(&p.key).await {
            Ok(Some(v)) => Ok(Json(serde_json::json!({ "hit": true, "value": v }))),
            Ok(None) => Ok(Json(serde_json::json!({ "hit": false }))),
            Err(e) => Err(InternalServerErrorException::new(e.message)),
        }
    }

    #[get("/ttl/:key")]
    pub async fn key_ttl(
        State(cache): State<Arc<CacheService>>,
        #[param::param] p: KeyParams,
    ) -> Json<serde_json::Value> {
        match cache.ttl(&p.key).await {
            Some(d) => Json(serde_json::json!({ "ttl_ms": d.as_millis() as u64 })),
            None => Json(serde_json::json!({ "ttl_ms": null })),
        }
    }

    #[delete("/del/:key")]
    pub async fn del_key(
        State(cache): State<Arc<CacheService>>,
        #[param::param] p: KeyParams,
    ) -> Json<serde_json::Value> {
        let existed = cache.del(&p.key).await;
        Json(serde_json::json!({ "deleted": existed }))
    }
}

#[module(
    imports = [CacheModule::register(CacheOptions::Redis(
        nestrs::RedisCacheOptions::new("redis://127.0.0.1:6380").with_prefix("lab5"),
    ))],
    controllers = [CacheController]
)]
pub struct LabModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .listen_graceful(3500)
        .await;
}
