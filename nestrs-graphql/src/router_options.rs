//! HTTP surface options for the GraphQL mount.

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql::{BatchRequest, ObjectType, Schema, SubscriptionType};
use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Router};

/// Controls GET/Playground vs POST-only GraphQL.
#[derive(Clone, Debug)]
pub struct GraphQlHttpOptions {
    /// When `true`, `GET` serves GraphQL Playground; when `false`, only `POST` is accepted.
    pub enable_playground: bool,
}

impl Default for GraphQlHttpOptions {
    fn default() -> Self {
        Self {
            enable_playground: true,
        }
    }
}

/// Same as [`crate::graphql_router`] with explicit HTTP options.
pub fn graphql_router_with_options<Q, M, S>(
    schema: Schema<Q, M, S>,
    path: impl Into<String>,
    options: GraphQlHttpOptions,
) -> Router
where
    Q: ObjectType + Send + Sync + 'static,
    M: ObjectType + Send + Sync + 'static,
    S: SubscriptionType + Send + Sync + 'static,
{
    let path = path.into();
    let endpoint = path.clone();

    if options.enable_playground {
        let playground = move || async move {
            Html(playground_source(GraphQLPlaygroundConfig::new(
                endpoint.as_str(),
            )))
        };
        Router::new()
            .route(
                path.as_str(),
                axum::routing::get(playground).post(graphql_handler::<Q, M, S>),
            )
            .layer(Extension(schema))
    } else {
        Router::new()
            .route(
                path.as_str(),
                axum::routing::get(|| async { StatusCode::METHOD_NOT_ALLOWED })
                    .post(graphql_handler::<Q, M, S>),
            )
            .layer(Extension(schema))
    }
}

pub(super) async fn graphql_handler<Q, M, S>(
    Extension(schema): Extension<Schema<Q, M, S>>,
    Json(req): Json<BatchRequest>,
) -> Response
where
    Q: ObjectType + Send + Sync + 'static,
    M: ObjectType + Send + Sync + 'static,
    S: SubscriptionType + Send + Sync + 'static,
{
    let resp = schema.execute_batch(req).await;
    let headers = resp.http_headers_iter().collect::<Vec<_>>();

    let mut http_resp = Json(resp).into_response();
    for (name, value) in headers {
        http_resp.headers_mut().append(name, value);
    }
    http_resp
}
