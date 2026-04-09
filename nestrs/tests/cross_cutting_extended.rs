use axum::http::request::Parts;
use nestrs::prelude::*;

#[roles("admin")]
#[set_metadata("scope", "users:read")]
#[allow(dead_code)]
fn decorator_marker_compile_check() {}

struct TestJwtStrategy;

#[async_trait]
impl AuthStrategy for TestJwtStrategy {
    type Payload = &'static str;

    async fn validate(&self, parts: &Parts) -> Result<Self::Payload, AuthError> {
        let ok = parts
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "Bearer test-token")
            .unwrap_or(false);
        if ok {
            Ok("user-1")
        } else {
            Err(AuthError::unauthorized("invalid auth"))
        }
    }
}

#[tokio::test]
async fn auth_strategy_trait_validates_headers() {
    let req = axum::http::Request::builder()
        .uri("/")
        .header("authorization", "Bearer test-token")
        .body(())
        .expect("request");
    let (parts, _body) = req.into_parts();

    let strategy = TestJwtStrategy;
    let payload = strategy.validate(&parts).await.expect("valid token");
    assert_eq!(payload, "user-1");
}

#[test]
fn metadata_registry_stores_and_reads_values() {
    MetadataRegistry::set("users::list", "roles", "admin");
    let role = MetadataRegistry::get("users::list", "roles").expect("metadata value");
    assert_eq!(role, "admin");
}
