use crate::options::IdempotencyOptions;
use crate::store::IdempotencyRecord;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Axum middleware: replay stored responses for repeated `Idempotency-Key` values.
///
/// The store key is bound to [`IdempotencyOptions::scope`] (when set) and to a
/// hash of the `Authorization` and `Cookie` headers, so one caller cannot read
/// another caller's cached body by presenting the same key. A completed key is
/// replayed only when method, path, query, and body match the original request;
/// a reuse against a different request is **409** and does not return the
/// stored body.
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
        .map(str::to_owned)
        .filter(|k| !k.trim().is_empty());

    let Some(key) = key else {
        if opts.required {
            return json_error(
                StatusCode::BAD_REQUEST,
                "Idempotency-Key required",
                "Bad Request",
                "IDEMPOTENCY_KEY_REQUIRED",
            );
        }
        return next.run(req).await;
    };

    let (parts, body) = req.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "Could not read request body",
                "Bad Request",
                "IDEMPOTENCY_BODY",
            );
        }
    };
    let caller = caller_hash(&parts.headers);
    let fingerprint = request_fingerprint(&parts.method, &parts.uri, &bytes, &caller);
    let scope_key = storage_key(opts.scope.as_deref(), &caller, &key);
    let req = axum::extract::Request::from_parts(parts, Body::from(bytes));

    match opts.store.begin_or_get(&scope_key).await {
        Ok(Some(rec)) => {
            if rec.fingerprint.is_empty() || rec.fingerprint != fingerprint {
                return json_error(
                    StatusCode::CONFLICT,
                    "Idempotency key was already used for a different request",
                    "Conflict",
                    "IDEMPOTENCY_KEY_MISMATCH",
                );
            }
            let mut res = Response::builder()
                .status(StatusCode::from_u16(rec.status).unwrap_or(StatusCode::OK))
                .body(Body::from(rec.body))
                .unwrap();
            if let Ok(ct) = HeaderValue::from_str(&rec.content_type) {
                res.headers_mut()
                    .insert(axum::http::header::CONTENT_TYPE, ct);
            }
            res.headers_mut()
                .insert("idempotency-replayed", HeaderValue::from_static("true"));
            return res;
        }
        Err(_) => {
            return json_error(
                StatusCode::CONFLICT,
                "Idempotency key in flight",
                "Conflict",
                "IDEMPOTENCY_IN_FLIGHT",
            );
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
        fingerprint,
    };
    opts.store.complete(&scope_key, record).await;

    Response::from_parts(parts, Body::from(bytes))
}

fn json_error(status: StatusCode, message: &str, error: &str, code: &str) -> Response {
    let body = format!(
        r#"{{"statusCode":{},"message":"{message}","error":"{error}","errorCode":"{code}"}}"#,
        status.as_u16()
    );
    Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

/// Partition the store by optional app scope, caller credentials, and the
/// client key. The caller portion is a hash so a bearer token is not copied
/// into the store key.
fn storage_key(scope: Option<&str>, caller: &str, idem_key: &str) -> String {
    match scope.map(str::trim).filter(|s| !s.is_empty()) {
        Some(scope) => format!("{scope}\u{1e}{caller}\u{1e}{idem_key}"),
        None => format!("{caller}\u{1e}{idem_key}"),
    }
}

fn caller_hash(headers: &HeaderMap) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"nestrs-idempotency-caller-v1");
    hasher.update(header_bytes(headers, axum::http::header::AUTHORIZATION));
    hasher.update([0xff]);
    hasher.update(header_bytes(headers, axum::http::header::COOKIE));
    hex_encode(&hasher.finalize())
}

fn header_bytes(headers: &HeaderMap, name: axum::http::HeaderName) -> &[u8] {
    headers.get(name).map(|v| v.as_bytes()).unwrap_or(b"")
}

fn request_fingerprint(method: &Method, uri: &Uri, body: &[u8], caller: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(method.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(uri.path().as_bytes());
    hasher.update([0]);
    hasher.update(uri.query().unwrap_or("").as_bytes());
    hasher.update([0]);
    hasher.update(body);
    hasher.update([0]);
    hasher.update(caller.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{IdempotencyStore, InMemoryIdempotencyStore};
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tower::ServiceExt;

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
                    fingerprint: "fp".into(),
                },
            )
            .await;
        let again = store.begin_or_get("k").await.unwrap().unwrap();
        assert_eq!(again.body, b"ok");
        assert_eq!(again.fingerprint, "fp");
    }

    fn router(opts: Arc<IdempotencyOptions>) -> Router {
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_for_route = Arc::clone(&hits);
        Router::new()
            .route(
                "/who",
                post(move |headers: HeaderMap, body: Bytes| {
                    let hits = Arc::clone(&hits_for_route);
                    async move {
                        let n = hits.fetch_add(1, Ordering::SeqCst);
                        let who = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("anon");
                        format!("{n}|{who}|{}", String::from_utf8_lossy(&body))
                    }
                }),
            )
            .route("/other", post(|| async { "other-secret" }))
            .layer(axum::middleware::from_fn_with_state(
                opts,
                idempotency_middleware,
            ))
    }

    fn request(uri: &str, key: &str, authorization: &str, body: &'static str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("idempotency-key", key)
            .header("authorization", authorization)
            .body(Body::from(body))
            .unwrap()
    }

    async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Option<String>, String) {
        let res = app.clone().oneshot(req).await.expect("response");
        let status = res.status();
        let replayed = res
            .headers()
            .get("idempotency-replayed")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            replayed,
            String::from_utf8_lossy(&body).into_owned(),
        )
    }

    #[tokio::test]
    async fn same_caller_and_request_replays_without_running_again() {
        let opts = Arc::new(IdempotencyOptions::in_memory(Duration::from_secs(60)));
        let app = router(opts);
        let first = send(&app, request("/who", "k", "Bearer a", "payload")).await;
        let second = send(&app, request("/who", "k", "Bearer a", "payload")).await;
        assert_eq!(first.0, StatusCode::OK);
        assert!(first.1.is_none());
        assert_eq!(second.0, StatusCode::OK);
        assert_eq!(second.1.as_deref(), Some("true"));
        assert_eq!(first.2, second.2);
        assert!(first.2.starts_with("0|Bearer a|payload"));
    }

    #[tokio::test]
    async fn different_caller_does_not_receive_the_cached_body() {
        let opts = Arc::new(IdempotencyOptions::in_memory(Duration::from_secs(60)));
        let app = router(opts);
        let first = send(&app, request("/who", "shared", "Bearer a", "payload")).await;
        let second = send(&app, request("/who", "shared", "Bearer b", "payload")).await;
        assert!(first.2.contains("Bearer a"));
        assert!(
            second.2.contains("Bearer b"),
            "second caller must execute as themselves, got {}",
            second.2
        );
        assert!(second.1.is_none());
        assert_ne!(first.2, second.2);
    }

    #[tokio::test]
    async fn same_key_on_a_different_request_does_not_leak_the_stored_body() {
        let opts = Arc::new(IdempotencyOptions::in_memory(Duration::from_secs(60)));
        let app = router(opts);
        let first = send(&app, request("/who", "k", "Bearer a", "payload")).await;
        let mismatch = send(&app, request("/other", "k", "Bearer a", "payload")).await;
        assert_eq!(first.0, StatusCode::OK);
        assert_eq!(mismatch.0, StatusCode::CONFLICT);
        assert!(mismatch.1.is_none());
        assert!(mismatch.2.contains("IDEMPOTENCY_KEY_MISMATCH"));
        assert!(!mismatch.2.contains("Bearer a"));
        assert!(!mismatch.2.contains("other-secret"));
    }

    #[tokio::test]
    async fn same_key_with_a_different_body_does_not_replay() {
        let opts = Arc::new(IdempotencyOptions::in_memory(Duration::from_secs(60)));
        let app = router(opts);
        let first = send(&app, request("/who", "k", "Bearer a", "one")).await;
        let mismatch = send(&app, request("/who", "k", "Bearer a", "two")).await;
        assert!(first.2.contains("|one"));
        assert_eq!(mismatch.0, StatusCode::CONFLICT);
        assert!(!mismatch.2.contains("|one"));
        assert!(mismatch.2.contains("IDEMPOTENCY_KEY_MISMATCH"));
    }
}
