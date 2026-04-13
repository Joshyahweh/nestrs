/// Multipart upload helpers (NestJS file upload building block).
///
/// This module re-exports Axum's multipart extractor so applications can handle uploads without
/// adding a direct `axum` dependency. Prefer [`axum::extract::Multipart`] (also in [`crate::prelude`]).
use crate::{BadRequestException, HttpException};
pub use axum::extract::multipart::{Field, MultipartError};

impl From<MultipartError> for HttpException {
    fn from(value: MultipartError) -> Self {
        BadRequestException::new(format!("Invalid multipart payload: {value}"))
    }
}
