use axum::body::to_bytes;
use nestrs::prelude::*;
use serde::Serialize;

#[derive(Default)]
#[injectable]
struct AppState;

#[derive(Serialize)]
struct UserDto {
    name: String,
}

#[controller]
struct SerializationController;

#[routes(state = AppState)]
impl SerializationController {
    #[get("/user")]
    #[serialize]
    async fn user() -> UserDto {
        UserDto {
            name: "alice".to_string(),
        }
    }

    #[get("/user_result")]
    #[serialize]
    async fn user_result() -> Result<UserDto, HttpException> {
        Ok(UserDto {
            name: "bob".to_string(),
        })
    }

    #[get("/user_error")]
    #[serialize]
    async fn user_error() -> Result<UserDto, HttpException> {
        Err(BadRequestException::new("nope"))
    }
}

#[module(controllers = [SerializationController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn serialize_macro_wraps_ok_values_in_json() {
    let app = TestingModule::builder::<AppModule>().compile().await;
    let client = app.http_client();

    let res = client.get("/user").send().await;
    assert_eq!(res.status(), 200);
    let ct = res
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("application/json"), "unexpected ct: {ct}");
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "alice");

    let res = client.get("/user_result").send().await;
    assert_eq!(res.status(), 200);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "bob");

    let res = client.get("/user_error").send().await;
    assert_eq!(res.status(), 400);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["error"], "Bad Request");
}
