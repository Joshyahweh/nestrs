use crate::{BadRequestException, HttpException};

/// Extracts the full request body as bytes (webhook-friendly raw body).
///
/// Pair with the `#[raw_body]` marker attribute on handlers for Nest-like readability.
pub struct RawBody(pub axum::body::Bytes);

#[axum::async_trait]
impl<S> axum::extract::FromRequest<S> for RawBody
where
    S: Send + Sync + 'static,
{
    type Rejection = HttpException;

    async fn from_request(
        req: axum::extract::Request,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
            .await
            .map_err(|e| BadRequestException::new(format!("Invalid request body: {e}")))?;
        Ok(Self(bytes))
    }
}
