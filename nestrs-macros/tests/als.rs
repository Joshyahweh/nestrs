//! Macro expansion tests for `#[nestrs_macros::als]`.
//!
//! Verifies that the macro generates:
//! - a `task_local!` cell with the right shape,
//! - `with_*` / `current_*` helpers that read/write the cell,
//! - an `axum::extract::FromRequestParts` impl returning `AlsError` on
//!   absence.
//!
//! The tests live in `nestrs-macros/tests/` so they exercise the
//! macro as a real user would (a crate that depends on both
//! `nestrs-macros` and `nestrs-core`, with `tokio` and `axum` as
//! transitive deps).

use axum::extract::FromRequestParts;
use nestrs_core::als::AlsError;
use nestrs_macros::als;

#[derive(Clone, Debug, PartialEq)]
#[als]
pub struct RequestContext {
    pub user_id: u32,
    pub tenant: String,
}

#[derive(Clone, Debug, PartialEq)]
#[als]
pub struct SingleField {
    pub value: i64,
}

#[derive(Clone, Debug, PartialEq)]
#[als]
pub struct TupleCtx(pub u32, pub String);

#[derive(Clone, Debug, PartialEq)]
#[als]
pub struct UnitCtx;

#[tokio::test]
async fn with_helper_installs_value_for_future_duration() {
    assert!(RequestContext::current_request_context().is_none());

    let inside = RequestContext::with_request_context(
        RequestContext {
            user_id: 7,
            tenant: "acme".into(),
        },
        async { RequestContext::current_request_context() },
    )
    .await;
    assert_eq!(
        inside,
        Some(RequestContext {
            user_id: 7,
            tenant: "acme".into()
        })
    );

    assert!(
        RequestContext::current_request_context().is_none(),
        "scope ended, value cleared"
    );
}

#[tokio::test]
async fn current_helper_returns_none_off_scope() {
    assert!(RequestContext::current_request_context().is_none());
    assert!(SingleField::current_single_field().is_none());
    assert!(TupleCtx::current_tuple_ctx().is_none());
    assert!(UnitCtx::current_unit_ctx().is_none());
}

#[tokio::test]
async fn nested_with_scopes_join_the_outer() {
    let outer = RequestContext::with_request_context(
        RequestContext {
            user_id: 1,
            tenant: "outer".into(),
        },
        async {
            assert_eq!(
                RequestContext::current_request_context().map(|c| c.user_id),
                Some(1)
            );

            // Inner scope: outer value must still be visible UNTIL
            // the inner install. After the inner scope ends, the
            // outer value comes back.
            let inner_observed = RequestContext::with_request_context(
                RequestContext {
                    user_id: 2,
                    tenant: "inner".into(),
                },
                async {
                    RequestContext::current_request_context().map(|c| c.user_id)
                },
            )
            .await;
            assert_eq!(inner_observed, Some(2));

            RequestContext::current_request_context().map(|c| c.user_id)
        },
    )
    .await;
    assert_eq!(outer, Some(1));
}

#[tokio::test]
async fn extractor_reads_value_installed_by_middleware() {
    let result = RequestContext::with_request_context(
        RequestContext {
            user_id: 42,
            tenant: "via-extractor".into(),
        },
        async {
            // The generated FromRequestParts impl reads from the
            // task-local cell. We don't need a real `Request` — the
            // impl ignores the parts entirely.
            let mut parts = axum::http::Request::new(()).into_parts().0;
            let state = ();
            RequestContext::from_request_parts(&mut parts, &state).await
        },
    )
    .await;
    assert_eq!(
        result,
        Ok(RequestContext {
            user_id: 42,
            tenant: "via-extractor".into()
        })
    );
}

#[tokio::test]
async fn extractor_rejects_when_value_is_not_installed() {
    let mut parts = axum::http::Request::new(()).into_parts().0;
    let state = ();
    let result = RequestContext::from_request_parts(&mut parts, &state).await;
    assert_eq!(result, Err(AlsError::NotSet));
}

#[tokio::test]
async fn extractor_works_for_single_field_struct() {
    let result = SingleField::with_single_field(
        SingleField { value: -1 },
        async {
            let mut parts = axum::http::Request::new(()).into_parts().0;
            SingleField::from_request_parts(&mut parts, &()).await
        },
    )
    .await;
    assert_eq!(result, Ok(SingleField { value: -1 }));
}

#[tokio::test]
async fn extractor_works_for_tuple_struct() {
    let result = TupleCtx::with_tuple_ctx(
        TupleCtx(99, "tuple".into()),
        async {
            let mut parts = axum::http::Request::new(()).into_parts().0;
            TupleCtx::from_request_parts(&mut parts, &()).await
        },
    )
    .await;
    assert_eq!(result, Ok(TupleCtx(99, "tuple".into())));
}

#[tokio::test]
async fn extractor_works_for_unit_struct() {
    let result = UnitCtx::with_unit_ctx(UnitCtx, async {
        let mut parts = axum::http::Request::new(()).into_parts().0;
        UnitCtx::from_request_parts(&mut parts, &()).await
    })
    .await;
    assert_eq!(result, Ok(UnitCtx));
}

#[tokio::test]
async fn extractor_rejects_unit_struct_when_not_set() {
    let mut parts = axum::http::Request::new(()).into_parts().0;
    let result = UnitCtx::from_request_parts(&mut parts, &()).await;
    assert_eq!(result, Err(AlsError::NotSet));
}

#[tokio::test]
async fn two_distinct_als_types_dont_interfere() {
    // Both ALS values live in independent cells; setting one must
    // not affect reads on the other.
    let _ = RequestContext::with_request_context(
        RequestContext {
            user_id: 1,
            tenant: "a".into(),
        },
        async {
            assert_eq!(
                SingleField::current_single_field(),
                None,
                "SingleField ALS untouched"
            );
            assert_eq!(
                RequestContext::current_request_context().map(|c| c.tenant),
                Some("a".to_string())
            );
        },
    )
    .await;
}
