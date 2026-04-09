//! Optional OpenAPI primitives for nestrs (Phase 4 roadmap crate).

pub struct OpenApiDocument(pub serde_json::Value);

impl OpenApiDocument {
    pub fn empty() -> Self {
        Self(serde_json::json!({
            "openapi": "3.1.0",
            "info": { "title": "nestrs API", "version": "0.1.0" },
            "paths": {}
        }))
    }
}

pub trait OpenApiProvider {
    fn openapi_document(&self) -> OpenApiDocument;
}
