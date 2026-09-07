#![cfg(feature = "database-sqlx")]

//! `TransactionSlot` + `install_transactional_middleware` integration tests.
//! Covers Feature E (ambient ORM transactions).

use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::routing::{get, post};
use axum::Router;
use nestrs::{
    current_transaction, install_transactional_middleware, TransactionSlot,
    TransactionalInterceptor,
};
use std::sync::Arc;
use tower::util::ServiceExt;

/// Fresh on-disk SQLite pool per test (matches the repository test fixture).
async fn fresh_pool() -> Arc<sqlx::AnyPool> {
    nestrs::install_default_drivers();
    let path = std::env::temp_dir().join(format!(
        "nestrs-tx-tests-{}-{}.sqlite",
        std::process::id(),
        uuid_like(),
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = sqlx::any::AnyPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect sqlite");
    let pool = Arc::new(pool);
    sqlx::query(
        "CREATE TABLE accounts (id INTEGER PRIMARY KEY AUTOINCREMENT, balance INTEGER NOT NULL)",
    )
    .execute(pool.as_ref())
    .await
    .expect("create accounts");
    pool
}

fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{n}")
}

async fn insert_balance(pool: &sqlx::AnyPool, balance: i64) -> i64 {
    let row = sqlx::query("INSERT INTO accounts (balance) VALUES ($1) RETURNING id")
        .bind(balance)
        .fetch_one(pool)
        .await
        .expect("insert");
    use sqlx::Row;
    row.try_get::<i64, _>("id").unwrap()
}

async fn read_balance(pool: &sqlx::AnyPool, id: i64) -> i64 {
    use sqlx::Row;
    let row = sqlx::query("SELECT balance FROM accounts WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("select");
    row.try_get::<i64, _>("balance").unwrap()
}

#[tokio::test]
async fn transactional_commits_on_2xx() {
    let pool = fresh_pool().await;
    let id = insert_balance(&pool, 100).await;

    let app = Router::new()
        .route(
            "/inc",
            post(move || async move {
                let slot = current_transaction().expect("slot");
                let result: Result<(), sqlx::Error> = slot
                    .with_tx(|tx| {
                        Box::pin(async move {
                            sqlx::query("UPDATE accounts SET balance = balance + 50 WHERE id = $1")
                                .bind(id)
                                .execute(&mut **tx)
                                .await?;
                            Ok(())
                        })
                    })
                    .await;
                result.expect("update");
                (StatusCode::OK, "ok")
            }),
        )
        .layer(from_fn_with_state(
            pool.clone(),
            install_transactional_middleware,
        ));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/inc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(read_balance(&pool, id).await, 150);
}

#[tokio::test]
async fn transactional_rolls_back_on_5xx() {
    let pool = fresh_pool().await;
    let id = insert_balance(&pool, 100).await;

    let app = Router::new()
        .route(
            "/inc-then-fail",
            post(move || async move {
                let slot = current_transaction().expect("slot");
                let result: Result<(), sqlx::Error> = slot
                    .with_tx(|tx| {
                        Box::pin(async move {
                            sqlx::query("UPDATE accounts SET balance = balance + 50 WHERE id = $1")
                                .bind(id)
                                .execute(&mut **tx)
                                .await?;
                            Ok(())
                        })
                    })
                    .await;
                result.expect("update");
                (StatusCode::INTERNAL_SERVER_ERROR, "boom")
            }),
        )
        .layer(from_fn_with_state(
            pool.clone(),
            install_transactional_middleware,
        ));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/inc-then-fail")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    // Rolled back; balance is still 100.
    assert_eq!(read_balance(&pool, id).await, 100);
}

#[tokio::test]
async fn transactional_does_not_open_tx_when_middleware_not_installed() {
    let pool = fresh_pool().await;
    let id = insert_balance(&pool, 100).await;

    // No install_transactional_middleware → current_transaction() is None.
    let app = Router::new().route(
        "/maybe",
        get(|| async move {
            assert!(current_transaction().is_none());
            (StatusCode::OK, "no-tx")
        }),
    );

    let res = app
        .oneshot(
            HttpRequest::builder()
                .uri("/maybe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    // Sanity: the id from the direct-insert fixture is untouched.
    assert_eq!(read_balance(&pool, id).await, 100);
}

#[tokio::test]
async fn transactional_works_with_interceptor_wrapper() {
    // Drive install_transactional_middleware via the TransactionalInterceptor
    // shim, with the pool in request extensions.
    let pool = fresh_pool().await;
    let id = insert_balance(&pool, 200).await;

    let pool_for_stash = pool.clone();
    let stash = move |req: axum::extract::Request, next: Next| {
        let pool = pool_for_stash.clone();
        async move {
            let (mut parts, body) = req.into_parts();
            parts.extensions.insert(pool);
            let req = axum::extract::Request::from_parts(parts, body);
            next.run(req).await
        }
    };

    let app = Router::new()
        .route(
            "/inc",
            post(|| async {
                let slot = current_transaction().expect("slot");
                let id = 1_i64; // The row we just inserted is always id=1 here.
                let result: Result<(), sqlx::Error> = slot
                    .with_tx(|tx| {
                        Box::pin(async move {
                            sqlx::query("UPDATE accounts SET balance = balance + 25 WHERE id = $1")
                                .bind(id)
                                .execute(&mut **tx)
                                .await?;
                            Ok(())
                        })
                    })
                    .await;
                if let Err(e) = result {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("update failed: {e}"),
                    );
                }
                (StatusCode::OK, "ok".to_string())
            }),
        )
        // Interceptor FIRST in code (applied first) → outermost, runs first;
        // stash SECOND (applied last) → innermost, but must run first since
        // it must inject the pool into extensions BEFORE the interceptor
        // reads it. Flip the order: apply stash last so it wraps the
        // handler, then apply the interceptor last so it wraps the stash.
        .layer(nestrs::interceptor_layer!(TransactionalInterceptor))
        .layer(from_fn_with_state((), stash));

    let res = app
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/inc")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "body: {:?}", {
        let body = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&body).to_string()
    });
    assert_eq!(read_balance(&pool, id).await, 225);
}

#[tokio::test]
async fn transactional_slot_commit_then_double_commit_is_noop() {
    // Once commit() runs, a second call should not panic or return an error.
    // Direct unit-level test of TransactionSlot semantics.
    let pool = fresh_pool().await;
    let tx = pool.begin().await.expect("begin");
    let slot = Arc::new(TransactionSlot::new(tx));
    slot.commit().await.expect("first commit");
    // Second commit: inner tx is already None, so the body of commit() is skipped.
    slot.commit().await.expect("second commit is a no-op");
    // And rollback after commit is also a no-op.
    slot.rollback()
        .await
        .expect("rollback after commit is a no-op");
}

#[tokio::test]
async fn transactional_slot_rollback_then_double_rollback_is_noop() {
    let pool = fresh_pool().await;
    let tx = pool.begin().await.expect("begin");
    let slot = Arc::new(TransactionSlot::new(tx));
    slot.rollback().await.expect("first rollback");
    slot.rollback().await.expect("second rollback is no-op");
}
