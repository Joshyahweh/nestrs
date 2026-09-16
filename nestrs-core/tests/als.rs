//! Integration tests for `nestrs_core::als` (the runtime surface the
//! `#[als]` proc-macro depends on).
//!
//! Run with `cargo test -p nestrs-core --test als`. The runtime is
//! always-on (no feature gate) — the macro is in `nestrs-macros` and
//! has its own tests there.

use nestrs_core::als::{AlsContext, AlsError};

tokio::task_local! {
    static CTX_KEY: Option<String> = const { None };
}

#[tokio::test]
async fn als_context_with_installs_value_for_future_duration() {
    let als = AlsContext::new(&CTX_KEY);
    assert!(als.current().is_none(), "empty before scope");

    let observed = als
        .with(String::from("hello"), async { als.current() })
        .await;
    assert_eq!(observed.as_deref(), Some("hello"));

    assert!(als.current().is_none(), "empty after scope ends");
}

#[tokio::test]
async fn als_context_with_nested_scopes_join_the_outer() {
    let als = AlsContext::new(&CTX_KEY);
    let observed = als
        .with(String::from("outer"), async {
            // Inner scope: pre-existing value must still be visible.
            let inside_outer = als.current();
            assert_eq!(inside_outer.as_deref(), Some("outer"));

            // Nested `with` opens an inner scope — the macro and the
            // runtime helper both follow tokio::task_local semantics,
            // where `.scope(value, future)` runs `future` with `value`
            // installed but the outer scope is restored after.
            let inner_observed = als
                .with(String::from("inner"), async { als.current() })
                .await;
            assert_eq!(inner_observed.as_deref(), Some("inner"));

            // After the inner scope ends, the outer value is back.
            als.current()
        })
        .await;
    assert_eq!(observed.as_deref(), Some("outer"));
}

#[tokio::test]
async fn als_context_current_returns_none_off_scope() {
    let als = AlsContext::new(&CTX_KEY);
    assert!(als.current().is_none());
}

#[tokio::test]
async fn als_context_with_empty_string_round_trips() {
    let als = AlsContext::new(&CTX_KEY);
    let observed = als.with(String::new(), async { als.current() }).await;
    assert_eq!(observed.as_deref(), Some(""));
}

#[tokio::test]
async fn als_error_display_mentions_the_fix() {
    let err = AlsError::NotSet;
    let msg = format!("{err}");
    assert!(
        msg.contains("not installed"),
        "error message should hint at the wiring fix: {msg}"
    );
    assert!(
        msg.contains("with_"),
        "error message should reference the with_* helper name: {msg}"
    );
}

#[tokio::test]
async fn als_error_eq_supports_match_arms() {
    assert_eq!(AlsError::NotSet, AlsError::NotSet);
    let err: AlsError = AlsError::NotSet;
    let msg = match err {
        AlsError::NotSet => "matched",
    };
    assert_eq!(msg, "matched");
}
