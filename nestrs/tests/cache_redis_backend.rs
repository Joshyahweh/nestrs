#![cfg(feature = "cache-redis")]

use nestrs::prelude::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn test_redis_url() -> Option<String> {
    std::env::var("NESTRS_TEST_REDIS_URL").ok().filter(|s| !s.trim().is_empty())
}

fn unique_key(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_nanos();
    format!("{prefix}:{nanos}")
}

#[tokio::test]
async fn cache_redis_backend_round_trips_and_expires() {
    let Some(url) = test_redis_url() else {
        // Opt-in: set NESTRS_TEST_REDIS_URL to run this test.
        return;
    };

    let dm = CacheModule::register(CacheOptions::redis(url));
    let cache = dm.registry.get::<CacheService>();

    let key = unique_key("nestrs:test:cache");

    cache
        .set(&key, &serde_json::json!({"v": 1}), Some(Duration::from_millis(80)))
        .await
        .unwrap();

    let v: serde_json::Value = cache.get(&key).await.unwrap().unwrap();
    assert_eq!(v["v"], 1);
    assert!(cache.ttl(&key).await.is_some());

    tokio::time::sleep(Duration::from_millis(140)).await;
    assert!(cache.get_json(&key).await.is_none());
    assert!(cache.ttl(&key).await.is_none());

    // DEL should be idempotent even if already expired.
    let _ = cache.del(&key).await;
}

