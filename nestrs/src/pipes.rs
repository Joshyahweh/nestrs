//! Built-in [`PipeTransform`](crate::core::PipeTransform) implementations.

use crate::async_trait;
use crate::core::PipeTransform;
use crate::{BadRequestException, HttpException};
use validator::Validate;

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

/// Validates a value using [`validator::Validate`] (NestJS `ValidationPipe` analogue).
///
/// Most users will enable this via `#[use_pipes(ValidationPipe)]` together with `#[param::body]`,
/// `#[param::query]`, or `#[param::param]` so extraction runs validation automatically.
#[derive(Default)]
pub struct ValidationPipe;

#[async_trait]
impl<T> PipeTransform<T> for ValidationPipe
where
    T: Validate + Send + Sync + 'static,
{
    type Output = T;
    type Error = HttpException;

    async fn transform(&self, value: T) -> Result<Self::Output, Self::Error> {
        value.validate().map_err(|e| {
            // Keep this aligned with `ValidatedBody` / `ValidatedQuery` / `ValidatedPath`.
            let mut errors = Vec::new();
            for (field, field_errors) in e.field_errors() {
                let constraints = field_errors
                    .iter()
                    .map(|ve| {
                        let code = ve.code.to_string();
                        let message = ve
                            .message
                            .as_ref()
                            .map(|m| m.to_string())
                            .unwrap_or_else(|| code.clone());
                        (code, message)
                    })
                    .collect::<std::collections::HashMap<_, _>>();

                errors.push(serde_json::json!({
                    "field": field,
                    "constraints": constraints,
                }));
            }

            crate::UnprocessableEntityException::new("Validation failed")
                .with_details(serde_json::json!(errors))
        })?;
        Ok(value)
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

    #[tokio::test]
    async fn validation_pipe_accepts_valid() {
        #[derive(serde::Deserialize, Validate, Debug)]
        struct Dto {
            #[validate(length(min = 3))]
            name: String,
        }

        let dto = Dto {
            name: "abc".to_string(),
        };

        let out = ValidationPipe.transform(dto).await.expect("valid");
        assert_eq!(out.name, "abc");
    }

    #[tokio::test]
    async fn validation_pipe_rejects_invalid() {
        #[derive(serde::Deserialize, Validate, Debug)]
        struct Dto {
            #[validate(length(min = 3))]
            name: String,
        }

        let dto = Dto {
            name: "a".to_string(),
        };

        let err = ValidationPipe.transform(dto).await.expect_err("invalid");
        assert_eq!(err.status, axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }
}
