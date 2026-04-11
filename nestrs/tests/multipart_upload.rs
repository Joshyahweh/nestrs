use axum::body::{to_bytes, Body};
use nestrs::prelude::*;

#[derive(Default)]
#[injectable]
struct AppState;

#[controller]
struct UploadController;

#[routes(state = AppState)]
impl UploadController {
    #[post("/upload")]
    async fn upload(mut multipart: Multipart) -> Result<String, HttpException> {
        let mut out = Vec::new();
        while let Some(field) = multipart.next_field().await? {
            let name = field.name().unwrap_or("").to_string();
            let file_name = field.file_name().unwrap_or("").to_string();
            let bytes = field.bytes().await?;
            out.push(format!("{name}:{file_name}:{}", bytes.len()));
        }
        Ok(out.join(","))
    }
}

#[module(controllers = [UploadController], providers = [AppState])]
struct AppModule;

#[tokio::test]
async fn multipart_upload_round_trips_file_field() {
    let app = TestingModule::builder::<AppModule>().compile().await;
    let client = app.http_client();

    let boundary = "X-BOUNDARY";
    let body = format!(
        "--{boundary}\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"hello.txt\"\r\n\
Content-Type: text/plain\r\n\
\r\n\
hello world\r\n\
--{boundary}--\r\n",
    );

    let res = client
        .post("/upload")
        .header(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
                .unwrap(),
        )
        .body(Body::from(body))
        .send()
        .await;

    assert_eq!(res.status(), 200);
    let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("file:hello.txt:11"),
        "unexpected response body: {s}"
    );
}
