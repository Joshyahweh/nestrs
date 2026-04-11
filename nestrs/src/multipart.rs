pub use axum::extract::multipart::{Field, MultipartError};
/// Multipart upload helpers (NestJS file upload building block).
///
/// This module re-exports Axum's multipart extractor so applications can handle uploads without
/// adding a direct `axum` dependency.
pub use axum::extract::Multipart;

use crate::{BadRequestException, HttpException};

impl From<MultipartError> for HttpException {
    fn from(value: MultipartError) -> Self {
        BadRequestException::new(format!("Invalid multipart payload: {value}"))
    }
}
