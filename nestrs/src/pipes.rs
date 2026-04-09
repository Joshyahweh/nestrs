//! Built-in [`PipeTransform`](crate::core::PipeTransform) implementations.

use crate::async_trait;
use crate::core::PipeTransform;
use crate::{BadRequestException, HttpException};

/// Parses a decimal string into `i64` (e.g. after reading a path or query segment as `String`).
#[derive(Default)]
pub struct ParseIntPipe;

#[async_trait]
impl PipeTransform<String> for ParseIntPipe {
    type Output = i64;
    type Error = HttpException;

    async fn transform(&self, value: String) -> Result<Self::Output, Self::Error> {
        value
            .parse()
            .map_err(|_| BadRequestException::new("Validation failed (integer expected)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn parse_int_accepts_valid() {
        let n = ParseIntPipe
            .transform("42".to_string())
            .await
            .expect("parse");
        assert_eq!(n, 42);
    }

    #[tokio::test]
    async fn parse_int_rejects_invalid() {
        let err = ParseIntPipe
            .transform("nope".to_string())
            .await
            .expect_err("bad request");
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
    }
}
