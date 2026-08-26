//! Lab 3 — uploads & static files: multipart upload with size checks, streaming
//! download endpoint, static directory mount, and path-traversal resistance.
//!
//! Run: `cargo run -p lab --bin lab3_files`

use nestrs::prelude::*;
use std::sync::Arc;

const UPLOAD_DIR: &str = "/tmp/nestrs-lab-uploads";
const MAX_FILE_BYTES: usize = 64 * 1024;

#[injectable]
pub struct UploadService;

impl UploadService {
    async fn save(&self, name: &str, data: Vec<u8>) -> Result<usize, HttpException> {
        if data.len() > MAX_FILE_BYTES {
            return Err(BadRequestException::new(format!(
                "file exceeds {MAX_FILE_BYTES} byte limit"
            )));
        }
        tokio::fs::create_dir_all(UPLOAD_DIR)
            .await
            .map_err(|e| InternalServerErrorException::new(e.to_string()))?;
        let safe_name = std::path::Path::new(name)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| BadRequestException::new("invalid file name"))?;
        let path = format!("{UPLOAD_DIR}/{safe_name}");
        tokio::fs::write(&path, &data)
            .await
            .map_err(|e| InternalServerErrorException::new(e.to_string()))?;
        Ok(data.len())
    }
}

// HttpException carries inline detail storage, tripping clippy's result_large_err here.
#[allow(clippy::result_large_err)]
fn sanitize_download_name(raw: &str) -> Result<String, HttpException> {
    // Reject anything that could escape the upload dir: separators, dot segments.
    if raw.contains("..") || raw.contains('/') || raw.contains('\\') || raw.starts_with('.') {
        return Err(BadRequestException::new("illegal path"));
    }
    Ok(raw.to_string())
}

#[derive(serde::Deserialize)]
pub struct NameParams {
    name: String,
}

#[controller(prefix = "/io")]
pub struct IoController;

#[routes(state = UploadService)]
impl IoController {
    /// Multipart upload: curl -F file=@somefile
    #[post("/upload")]
    pub async fn upload(
        State(svc): State<Arc<UploadService>>,
        mut mp: axum::extract::Multipart,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let mut saved = Vec::new();
        while let Some(field) = mp.next_field().await.map_err(HttpException::from)? {
            let name = field.file_name().unwrap_or("unnamed").to_string();
            let data = field.bytes().await.map_err(HttpException::from)?.to_vec();
            let size = svc.save(&name, data).await?;
            saved.push(serde_json::json!({ "name": name, "bytes": size }));
        }
        Ok(Json(serde_json::json!({ "saved": saved })))
    }

    /// Streaming download of an uploaded file.
    #[get("/download/:name")]
    pub async fn download(
        #[param::param] p: NameParams,
    ) -> Result<axum::response::Response, HttpException> {
        let name = sanitize_download_name(&p.name)?;
        Ok(nestrs::stream_file_or_response(
            format!("{UPLOAD_DIR}/{name}"),
            "application/octet-stream",
        )
        .await)
    }
}

#[module(controllers = [IoController], providers = [UploadService])]
pub struct LabModule;

#[tokio::main]
async fn main() {
    // Seed a static dir with an index page.
    let static_dir = "/tmp/nestrs-lab-static";
    let _ = tokio::fs::create_dir_all(static_dir).await;
    let _ = tokio::fs::write(format!("{static_dir}/index.html"), "<h1>lab static</h1>").await;
    let _ = tokio::fs::create_dir_all(UPLOAD_DIR).await;

    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .serve_static("/static", static_dir)
        .listen_graceful(3300)
        .await;
}
