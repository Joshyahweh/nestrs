//! Per-request GraphQL handler context plumbing.
//!
//! This module is gated on the `graphql-authz` feature. It exposes the
//! *type-erased* pieces the GraphQL HTTP handler needs to install
//! per-request scopes (ability + transaction) before running the
//! schema execution:
//!
//! - [`GqlHandlerHook`]: trait implemented by the user (or by the
//!   main `nestrs` crate's typed `GqlDataContext`) that provides
//!   pre/post hooks around the schema execution.
//! - [`graphql_router_with_hook`]: builder that wires the hook into
//!   the existing handler.
//!
//! ## Why a trait, not a context struct
//!
//! `nestrs-graphql` cannot depend on `nestrs` (the parent framework
//! crate) without creating a Cargo cycle
//! (`nestrs -> nestrs-graphql -> nestrs`). It therefore cannot name
//! `nestrs::policies::Ability` or
//! `nestrs::transactional::TransactionSlot`. A trait object is the
//! natural way to let the main crate inject typed behaviour into a
//! generic handler.

#[cfg(feature = "graphql-authz")]
mod inner {
    use async_graphql::{BatchRequest, BatchResponse, ObjectType, Schema, SubscriptionType};
    use axum::extract::Json;
    use axum::http::StatusCode;
    use axum::response::{Html, IntoResponse};
    use axum::{Extension, Router};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    /// Per-request hook the GraphQL handler calls before and after
    /// `schema.execute_batch`. Implementations are responsible for:
    ///
    /// 1. Opening a transaction from any pool they hold (returning
    ///    `None` if no transaction is needed).
    /// 2. Stashing the ability + tx into the per-task scope
    ///    (`nestrs_core::with_ability_erased` /
    ///    `nestrs_core::request_scope_insert`).
    /// 3. Running the inner future.
    /// 4. Calling any post-mask on the response.
    ///
    /// The default no-op implementation in this module ([`NoopHook`])
    /// just runs the inner future with no pre/post work — the legacy
    /// path.
    pub trait GqlHandlerHook: Send + Sync + 'static {
        /// Run `execute` inside the hook's scope. The hook is
        /// responsible for any pre/post work; it returns the final
        /// `BatchResponse`.
        fn run<'a>(
            &'a self,
            execute: Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>,
        ) -> Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>;

        /// Called once per request in the batch, *before* schema
        /// execution, with a mutable handle on the request. Used to
        /// install per-request request-data — e.g. the `dataloader`
        /// feature's `DataLoaderRegistry` inserting fresh loaders
        /// (see `nestrs-graphql::data_loader`). Default: no-op.
        fn prepare(&self, _request: &mut async_graphql::Request) {}
    }

    /// Default no-op hook used by `graphql_router_with_options` (no
    /// extension layers needed — legacy path).
    #[derive(Default)]
    pub struct NoopHook;

    impl GqlHandlerHook for NoopHook {
        fn run<'a>(
            &'a self,
            execute: Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>,
        ) -> Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>> {
            Box::pin(execute)
        }
    }

    impl GqlHandlerHook for Arc<dyn GqlHandlerHook> {
        fn run<'a>(
            &'a self,
            execute: Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>>,
        ) -> Pin<Box<dyn Future<Output = BatchResponse> + Send + 'a>> {
            (**self).run(execute)
        }

        fn prepare(&self, request: &mut async_graphql::Request) {
            (**self).prepare(request)
        }
    }

    /// Build a `Router` that serves `schema` at `path` with a per-request
    /// `hook` installed around the schema execution. The hook is read
    /// from request extensions.
    ///
    /// `nestrs::graphql_authz::graphql_router_with_context` (in the main
    /// crate) is the typed wrapper that takes a `GqlDataContext`,
    /// wraps it in a `GqlHandlerHook` impl, and calls this builder.
    pub fn graphql_router_with_hook<Q, M, S>(
        schema: Schema<Q, M, S>,
        path: impl Into<String>,
        options: crate::GraphQlHttpOptions,
        hook: Arc<dyn GqlHandlerHook>,
    ) -> Router
    where
        Q: ObjectType + Send + Sync + 'static,
        M: ObjectType + Send + Sync + 'static,
        S: SubscriptionType + Send + Sync + 'static,
    {
        let path = path.into();
        let endpoint = path.clone();
        let schema_for_layer = schema;

        let handler = move |Extension(schema): Extension<Schema<Q, M, S>>,
                            Extension(hook): Extension<Arc<dyn GqlHandlerHook>>,
                            Json(req): Json<BatchRequest>| {
            let hook = hook.clone();
            async move {
                // Pre-execution hook: runs once per request in the batch
                // (e.g. the dataloader registry installs fresh loaders
                // into each request's data map). Runs before `execute` is
                // even constructed, so the requests can be mutated.
                let mut req = req;
                match &mut req {
                    BatchRequest::Single(request) => hook.prepare(request),
                    BatchRequest::Batch(requests) => {
                        for request in requests {
                            hook.prepare(request);
                        }
                    }
                }
                let execute = Box::pin(async move { schema.execute_batch(req).await });
                let resp = hook.run(execute).await;
                let headers = resp.http_headers_iter().collect::<Vec<_>>();
                let mut http_resp = Json(resp).into_response();
                for (name, value) in headers {
                    http_resp.headers_mut().append(name, value);
                }
                http_resp
            }
        };

        if options.enable_playground {
            let playground = move || async move {
                Html(async_graphql::http::playground_source(
                    async_graphql::http::GraphQLPlaygroundConfig::new(endpoint.as_str()),
                ))
            };
            Router::new()
                .route(path.as_str(), axum::routing::get(playground).post(handler))
                .layer(Extension(schema_for_layer))
                .layer(Extension(hook))
        } else {
            Router::new()
                .route(
                    path.as_str(),
                    axum::routing::get(|| async { StatusCode::METHOD_NOT_ALLOWED }).post(handler),
                )
                .layer(Extension(schema_for_layer))
                .layer(Extension(hook))
        }
    }
}

#[cfg(feature = "graphql-authz")]
pub use inner::{graphql_router_with_hook, GqlHandlerHook, NoopHook};
