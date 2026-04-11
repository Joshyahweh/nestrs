use axum::body::to_bytes;
use nestrs::prelude::*;
use std::path::PathBuf;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller(prefix = "/api")]
struct ApiController;

#[routes(state = AppState)]
impl ApiController {
    #[get("/ping")]
    async fn ping() -> &'static str {
        "pong"
    }
}

#[module(controllers = [ApiController], providers = [AppState])]
struct AppModule;

fn unique_tmp_dir(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    let unique = format!(
        "nestrs-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    dir.push(unique);
    dir
}

#[tokio::test]
async fn serve_static_mounts_at_root_not_under_global_prefix() {
    let dir = unique_tmp_dir("static");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello").unwrap();

    let app = TestingModule::builder::<AppModule>()
        .configure_http(move |app| {
            app.set_global_prefix("api")
                .serve_static("/public", dir.clone())
        })
        .compile()
        .await;

    let client = app.http_client();

    // Static file served at root.
    let res = client.get("/public/hello.txt").send().await;
    assert_eq!(res.status(), 200);
    let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    assert_eq!(std::str::from_utf8(&body).unwrap(), "hello");

    // Not under global prefix.
    let res = client.get("/api/public/hello.txt").send().await;
    assert_eq!(res.status(), 404);
}
