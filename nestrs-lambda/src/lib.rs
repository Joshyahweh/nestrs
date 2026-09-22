//! AWS Lambda adapter for nestrs — NestJS serverless / `aws-serverless-express` analogue.
//!
//! Pass the router from [`NestApplication::into_router`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.into_router)
//! to [`listen_lambda`].

#![doc(html_root_url = "https://docs.rs/nestrs-lambda/1.5.0")]

use axum::Router;

pub use lambda_http;

/// Serve a nestrs/Axum router on Lambda (API Gateway HTTP API, REST API, ALB, Function URL).
pub async fn listen_lambda(router: Router) -> Result<(), lambda_http::Error> {
    lambda_http::run(router).await
}

#[cfg(test)]
mod tests {
    use axum::Router;

    #[test]
    fn router_type_is_accepted() {
        let _router = Router::<()>::new();
    }
}
