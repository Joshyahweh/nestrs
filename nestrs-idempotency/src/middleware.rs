use crate::options::IdempotencyOptions;
use crate::store::IdempotencyRecord;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use http_body_util::BodyExt;
use std::sync::Arc;

/// Axum middleware: replay stored responses for repeated `Idempotency-Key` values.
pub async fn idempotency_middleware(
    State(opts): State<Arc<IdempotencyOptions>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let header = opts.header_name.as_str();
    let key = req
        .headers()
        .get(header)
        .or_else(|| req.headers().get("Idempotency-Key"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let Some(key) = key else {
        if opts.required {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"statusCode":400,"message":"Idempotency-Key required","error":"Bad Request","errorCode":"IDEMPOTENCY_KEY_REQUIRED"}"#,
                ))
                .unwrap();
        }
        return next.run(req).await;
    };

    let scope_key = match &opts.scope {
        Some(s) => format!("{s}:{key}"),
        None => key.clone(),
    };

    match opts.store.begin_or_get(&scope_key).await {
        Ok(Some(rec)) => {
            let mut res = Response::builder()
                .status(StatusCode::from_u16(rec.status).unwrap_or(StatusCode::OK))
                .body(Body::from(rec.body))
                .unwrap();
            if let Ok(ct) = HeaderValue::from_str(&rec.content_type) {
                res.headers_mut()
                    .insert(axum::http::header::CONTENT_TYPE, ct);
            }
            res.headers_mut().insert(
                "idempotency-replayed",
                HeaderValue::from_static("true"),
            );
            return res;
        }
        Err(_) => {
            return Response::builder()
                .status(StatusCode::CONFLICT)
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"statusCode":409,"message":"Idempotency key in flight","error":"Conflict","errorCode":"IDEMPOTENCY_IN_FLIGHT"}"#,
                ))
                .unwrap();
        }
        Ok(None) => {}
    }

    let response = next.run(req).await;
    let (parts, body) = response.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_else(|_| Bytes::new());
    let content_type = parts
        .headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();
    let record = IdempotencyRecord {
        status: parts.status.as_u16(),
        body: bytes.to_vec(),
        content_type,
    };
    opts.store.complete(&scope_key, record).await;

    Response::from_parts(parts, Body::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{IdempotencyStore, InMemoryIdempotencyStore};
    use std::time::Duration;

    #[tokio::test]
    async fn store_replays_completed() {
        let store = InMemoryIdempotencyStore::new(Duration::from_secs(60));
        assert!(store.begin_or_get("k").await.unwrap().is_none());
        store
            .complete(
                "k",
                IdempotencyRecord {
                    status: 200,
                    body: b"ok".to_vec(),
                    content_type: "text/plain".into(),
                },
            )
            .await;
        let again = store.begin_or_get("k").await.unwrap().unwrap();
        assert_eq!(again.body, b"ok");
    }
}
