//! GraphQL integration for `nestrs` using `async-graphql`.

pub mod limits;

pub use limits::{
    with_default_limits, Analyzer, DEFAULT_MAX_COMPLEXITY, DEFAULT_MAX_DEPTH,
};

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
pub use async_graphql::{BatchRequest, ObjectType, Schema, SubscriptionType};
use axum::extract::Json;
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Router};

pub fn graphql_router<Query, Mutation, Subscription>(
    schema: Schema<Query, Mutation, Subscription>,
    path: impl Into<String>,
) -> Router
where
    Query: ObjectType + Send + Sync + 'static,
    Mutation: ObjectType + Send + Sync + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    let path = path.into();
    let endpoint = path.clone();

    let playground = move || async move {
        Html(playground_source(GraphQLPlaygroundConfig::new(endpoint.as_str())))
    };

    Router::new()
        .route(
            path.as_str(),
            axum::routing::get(playground).post(graphql_handler::<Query, Mutation, Subscription>),
        )
        .layer(Extension(schema))
}

async fn graphql_handler<Query, Mutation, Subscription>(
    Extension(schema): Extension<Schema<Query, Mutation, Subscription>>,
    Json(req): Json<BatchRequest>,
) -> Response
where
    Query: ObjectType + Send + Sync + 'static,
    Mutation: ObjectType + Send + Sync + 'static,
    Subscription: SubscriptionType + Send + Sync + 'static,
{
    let resp = schema.execute_batch(req).await;
    let headers = resp.http_headers_iter().collect::<Vec<_>>();

    let mut http_resp = Json(resp).into_response();
    for (name, value) in headers {
        http_resp.headers_mut().append(name, value);
    }
    http_resp
}
