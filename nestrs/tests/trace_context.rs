//! Wave 3E.3 — W3C trace context ambient accessors.
//!
//! HTTP: `use_trace_context` parses `traceparent` and installs the ambient
//! task-local; `ExecutionContext::trace_id()` and `RequestContext::traceparent`
//! surface it. GraphQL: resolvers run inside the request task, so the same
//! slot is visible. MCP/WS hosts install the slot with `with_trace_context`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::ServiceExt;

const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

// ---------------------------------------------------------------------------
// HTTP stack
// ---------------------------------------------------------------------------

#[derive(Default)]
#[injectable]
struct TraceState;

#[controller(prefix = "/trace")]
struct TraceController;

impl TraceController {
    #[get("/exec")]
    async fn exec_ctx(ctx: nestrs::HttpExecutionContext) -> String {
        match (ctx.trace_id(), ctx.span_id(), ctx.trace_flags()) {
            (Some(t), Some(s), Some(f)) => format!("trace:{t} span:{s} flags:{f}"),
            _ => "none".to_string(),
        }
    }

    #[get("/raw")]
    async fn raw_ctx(rc: nestrs::RequestContext) -> String {
        format!(
            "tp:{} ts:{}",
            rc.traceparent.as_deref().unwrap_or("none"),
            rc.tracestate.as_deref().unwrap_or("none")
        )
    }
}

impl_routes!(TraceController, state TraceState => [
    GET "/exec" with () => TraceController::exec_ctx,
    GET "/raw" with () => TraceController::raw_ctx,
]);

#[module(
    imports = [],
    controllers = [TraceController],
    providers = [TraceState],
)]
struct TraceModule;

async fn get(app: axum::Router, uri: &str, traceparent: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some(tp) = traceparent {
        builder = builder
            .header("traceparent", tp)
            .header("tracestate", "acme=1");
    }
    let req = builder.body(Body::empty()).expect("build request");
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn http_handler_reads_trace_via_execution_context() {
    let app = NestFactory::create::<TraceModule>()
        .use_trace_context()
        .use_execution_context()
        .into_router();
    let (status, body) = get(app, "/trace/exec", Some(TRACEPARENT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body, "trace:4bf92f3577b34da6a3ce929d0e0e4736 span:00f067aa0ba902b7 flags:01",
        "ambient trace must be visible from the handler"
    );
}

#[tokio::test]
async fn http_without_traceparent_header_sees_none() {
    let app = NestFactory::create::<TraceModule>()
        .use_trace_context()
        .use_execution_context()
        .into_router();
    let (_, body) = get(app, "/trace/exec", None).await;
    assert_eq!(body, "none", "no header => no ambient trace context");
}

#[tokio::test]
async fn http_invalid_traceparent_is_ignored() {
    let app = NestFactory::create::<TraceModule>()
        .use_trace_context()
        .use_execution_context()
        .into_router();
    // Version ff is invalid per W3C — must not install a context.
    let (_, body) = get(
        app,
        "/trace/exec",
        Some("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
    )
    .await;
    assert_eq!(body, "none");
}

#[tokio::test]
async fn request_context_exposes_raw_traceparent_and_tracestate() {
    let app = NestFactory::create::<TraceModule>()
        .use_trace_context()
        .use_request_context()
        .into_router();
    let (_, body) = get(app, "/trace/raw", Some(TRACEPARENT)).await;
    assert_eq!(
        body,
        "tp:00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01 ts:acme=1"
    );
}

// ---------------------------------------------------------------------------
// GraphQL transport: resolvers run inside the request task, so the ambient
// trace installed by the HTTP middleware is visible from the resolver.
// ---------------------------------------------------------------------------

#[cfg(feature = "graphql-authz")]
mod gql {
    use super::*;
    use async_graphql::{EmptyMutation, EmptySubscription, Object, SimpleObject};
    use nestrs::graphql::GraphQlHttpOptions;
    use nestrs::{current_trace, graphql_router_with_context, GqlDataContext};
    use serde_json::{json, Value};

    #[derive(SimpleObject)]
    struct TraceMarker {
        trace_id: Option<String>,
        span_id: Option<String>,
    }

    #[derive(Default)]
    struct Query;

    #[Object]
    impl Query {
        async fn trace_marker(&self) -> TraceMarker {
            match current_trace() {
                Some(t) => TraceMarker {
                    trace_id: Some(t.trace_id_hex()),
                    span_id: Some(t.span_id_hex()),
                },
                None => TraceMarker {
                    trace_id: None,
                    span_id: None,
                },
            }
        }
    }

    #[tokio::test]
    async fn gql_resolver_reads_ambient_trace_context() {
        let schema = async_graphql::Schema::build(Query, EmptyMutation, EmptySubscription).finish();
        let app = graphql_router_with_context(
            schema,
            "/graphql",
            GraphQlHttpOptions::default(),
            GqlDataContext::new(),
        )
        .layer(axum::middleware::from_fn(
            nestrs::install_trace_context_middleware,
        ));
        let req = Request::builder()
            .method("POST")
            .uri("/graphql")
            .header("content-type", "application/json")
            .header("traceparent", TRACEPARENT)
            .body(Body::from(
                serde_json::to_vec(&json!({ "query": "query { traceMarker { traceId spanId } }" }))
                    .expect("encode"),
            ))
            .expect("build request");
        let resp = app.oneshot(req).await.expect("oneshot");
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("json");
        let marker = &value["data"]["traceMarker"];
        assert_eq!(
            marker["traceId"].as_str(),
            Some("4bf92f3577b34da6a3ce929d0e0e4736"),
            "trace id must survive into the resolver: {value}"
        );
        assert_eq!(marker["spanId"].as_str(), Some("00f067aa0ba902b7"));
    }
}

// ---------------------------------------------------------------------------
// Non-HTTP transports: hosts (MCP stdio, message consumers) install the slot
// with `with_trace_context`; nested futures running inside the scope (the
// shape of a tool-body dispatch) read it.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn with_trace_context_survives_into_nested_dispatch() {
    let ctx = nestrs::core::parse_traceparent(TRACEPARENT).expect("valid");
    let observed = nestrs::core::with_trace_context(ctx, async {
        // Stand-in for a tool body / consumer handler running inside the
        // installed scope.
        let t = nestrs::core::current_trace_context().expect("installed");
        format!(
            "{}:{}",
            t.trace_id_hex(),
            t.tracestate.as_deref().unwrap_or("-")
        )
    })
    .await;
    assert_eq!(
        observed, "4bf92f3577b34da6a3ce929d0e0e4736:-",
        "no tracestate was installed"
    );

    // With tracestate installed (as the WS handshake / HTTP middleware do).
    let mut ctx = nestrs::core::parse_traceparent(TRACEPARENT).expect("valid");
    ctx.tracestate = Some("acme=1".into());
    let observed = nestrs::core::with_trace_context(ctx, async {
        let t = nestrs::core::current_trace_context().expect("installed");
        t.tracestate.expect("tracestate")
    })
    .await;
    assert_eq!(observed, "acme=1");
}
