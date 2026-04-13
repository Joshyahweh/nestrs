use crate::core::ExecutionContext;
use axum::extract::Request;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::ops::Deref;

pub(crate) async fn install_execution_context_middleware(req: Request, next: Next) -> Response {
    let (mut parts, body) = req.into_parts();
    let ctx = ExecutionContext::from_http_parts(&parts);
    parts.extensions.insert(ctx);
    next.run(Request::from_parts(parts, body)).await
}

/// Axum extractor for [`ExecutionContext`] (Nest-style `ArgumentsHost` snapshot).
///
/// Enable with [`crate::NestApplication::use_execution_context`].
#[derive(Clone, Debug)]
pub struct HttpExecutionContext(pub ExecutionContext);

impl Deref for HttpExecutionContext {
    type Target = ExecutionContext;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Returned when [`HttpExecutionContext`] is used but the app did not enable
/// [`crate::NestApplication::use_execution_context`].
#[derive(Debug)]
pub struct ExecutionContextMissing;

impl IntoResponse for ExecutionContextMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: HttpExecutionContext extractor requires NestApplication::use_execution_context()",
        )
            .into_response()
    }
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for HttpExecutionContext
where
    S: Send + Sync,
{
    type Rejection = ExecutionContextMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<ExecutionContext>()
            .cloned()
            .map(HttpExecutionContext)
            .ok_or(ExecutionContextMissing)
    }
}
