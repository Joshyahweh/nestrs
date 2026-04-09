//! Global exception filters (Nest-style): rewrite responses built from [`crate::HttpException`].
//!
//! [`crate::HttpException::into_response`] stores a copy of the exception in the response
//! [`Extensions`](axum::http::Extensions). When you call [`crate::NestApplication::use_global_exception_filter`],
//! an inner middleware runs [`ExceptionFilter::catch`] for those responses before outer layers (CORS,
//! production error sanitization, etc.).

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;

/// Async hook invoked when a handler (or guard) produced an [`crate::HttpException`] response.
#[async_trait::async_trait]
pub trait ExceptionFilter: Send + Sync {
    async fn catch(&self, ex: crate::HttpException) -> Response;
}

pub(crate) async fn exception_filter_middleware(
    axum::extract::State(filter): axum::extract::State<Arc<dyn ExceptionFilter>>,
    req: Request,
    next: Next,
) -> Response {
    let res = next.run(req).await;
    if let Some(ex) = res.extensions().get::<crate::HttpException>().cloned() {
        filter.catch(ex).await
    } else {
        res
    }
}
