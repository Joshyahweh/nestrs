//! Async-local-storage runtime support for the `#[als]` proc-macro.
//!
//! Async-local storage (ALS) is the NestJS `cls-hooked` / `nestjs-cls`
//! pattern: a value that propagates through every `.await` on the
//! current task without being threaded through every function
//! signature. In Rust the underlying primitive is `tokio::task_local!`,
//! and `nestrs-macros::als` generates the per-type static cell, the
//! `with_*` / `current_*` helpers, and the `FromRequestParts` impl
//! around it. This module is the small runtime surface the macro
//! relies on:
//!
//! - [`AlsError`] — the rejection type the generated extractor
//!   returns when middleware forgot to install the value.
//! - [`task_local!`] — re-export of [`tokio::task_local!`] so the
//!   `#[als]` macro's emitted code has a stable path
//!   (`::nestrs_core::als::task_local!`) without requiring `tokio`
//!   as a direct user dependency.
//! - `async_trait` — re-export of `async_trait::async_trait` so the
//!   generated `FromRequestParts` impl can use axum 0.7's
//!   `async_trait`-based extractor trait without requiring
//!   `async-trait` as a direct user dependency.
//! - [`AlsCell`] — type alias for the `tokio::task_local!` cell the
//!   runtime helper wraps, so user code can name it without a
//!   tokio-internal type path.
//! - [`AlsContext`] — a small typed wrapper around a `tokio::task_local!`
//!   cell for users who don't want to use the proc-macro.
//!
//! That's intentionally a thin layer. The macro is the primary
//! surface; this module just gives its generated code something
//! stable to point at so callers can `match` on the failure mode.

/// Rejection type returned by an `#[als]`-generated extractor when
/// the value was never installed on the current task.
///
/// This almost always indicates a wiring bug — middleware that should
/// have set the ALS via `with_<name>(value, future)` didn't, so the
/// handler runs without the per-request context it expects. Surface
/// it as a 500 (or your framework's "internal error") so it doesn't
/// masquerade as a 400 / 404 from a real request shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlsError {
    /// No value for the requested ALS key is installed on the current
    /// task. Either the request never entered a `with_*` scope, or a
    /// `tokio::spawn` boundary stripped the task-local — see
    /// [`crate::spawn_with_request_scope`] for the request-scoped
    /// analogue.
    NotSet,
}

impl std::fmt::Display for AlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlsError::NotSet => f.write_str(
                "async-local-storage value is not installed on this task \
                 — middleware should set it via `with_<name>(value, future).await`",
            ),
        }
    }
}

impl std::error::Error for AlsError {}

impl axum::response::IntoResponse for AlsError {
    fn into_response(self) -> axum::response::Response {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            self.to_string(),
        )
            .into_response()
    }
}

/// Re-export of [`tokio::task_local!`] so the `#[als]` macro's emitted
/// code can reference the primitive via a stable path
/// (`::nestrs_core::als::task_local!`) without requiring `tokio` to be
/// a direct dependency of the user's crate.
pub use tokio::task_local;

/// Re-export of [`async_trait::async_trait`] so the `#[als]` macro's
/// generated `FromRequestParts` impl can use axum 0.7's
/// `async_trait`-based extractor trait without requiring `async-trait`
/// as a direct user dependency.
pub use async_trait::async_trait;

/// Type alias for the `tokio::task_local!` cell [`AlsContext`] wraps.
/// Exists so helper users can name the cell without a tokio-internal
/// type path.
pub type AlsCell<T> = tokio::task::LocalKey<std::cell::RefCell<Option<T>>>;

/// Manual async-local-storage helper for users who don't want to use
/// the `#[als]` proc-macro.
///
/// Wraps a `tokio::task_local!` cell with a small ergonomic surface:
/// install a value for the duration of a future, read the current
/// value from anywhere on the task, scope-guard that automatically
/// drops. The macro is sugar over this trait — pick whichever fits
/// your codebase.
///
/// # Example
///
/// ```ignore
/// use nestrs_core::als::{AlsContext, AlsError};
///
/// tokio::task_local! {
///     static MY_CTX: std::cell::RefCell<Option<String>>;
/// }
///
/// async fn handler() -> Result<String, AlsError> {
///     AlsContext::new(&MY_CTX).current().ok_or(AlsError::NotSet)
/// }
/// ```
pub struct AlsContext<T>
where
    T: Clone + Send + Sync + 'static,
{
    cell: &'static AlsCell<T>,
}

impl<T> AlsContext<T>
where
    T: Clone + Send + Sync + 'static,
{
    /// Bind a typed view to a `tokio::task_local!` cell.
    pub fn new(cell: &'static AlsCell<T>) -> Self {
        Self { cell }
    }

    /// Run `future` with `value` installed in the cell. After `future`
    /// completes (or panics), the cell is restored to its prior state.
    pub async fn with<F, R>(&self, value: T, future: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        // `scope` runs `future` with the cell set to a fresh `Some(value)`.
        // The drop semantics of `task_local` restore the prior state.
        self.cell
            .scope(std::cell::RefCell::new(Some(value)), future)
            .await
    }

    /// Read the current value. Returns `None` outside any `with` scope.
    pub fn current(&self) -> Option<T> {
        self.cell
            .try_with(|cell| cell.borrow().clone())
            .ok()
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    tokio::task_local! {
        static CTX: std::cell::RefCell<Option<String>>;
    }

    #[tokio::test]
    async fn als_context_with_installs_value_for_future_duration() {
        let als = AlsContext::new(&CTX);
        assert!(als.current().is_none(), "empty before scope");

        let inside = als
            .with(String::from("hello"), async { als.current() })
            .await;
        assert_eq!(inside.as_deref(), Some("hello"));

        assert!(als.current().is_none(), "empty after scope ends");
    }

    #[tokio::test]
    async fn als_context_with_restores_after_panic() {
        let als = AlsContext::new(&CTX);
        // Install once normally, then install a panicking future inside.
        // The outer cell must be restored.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            rt.block_on(async {
                let _ = als
                    .with(String::from("outer"), async {
                        // Inner scope swallows the panic via `catch_unwind`.
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .build()
                                .unwrap();
                            rt.block_on(async {
                                let _ = als
                                    .with(String::from("inner"), async {
                                        panic!("simulated panic");
                                    })
                                    .await;
                            });
                        }));
                        assert!(result.is_err(), "inner panic propagated");
                        als.current()
                    })
                    .await;
            });
        }));
        assert!(als.current().is_none(), "outer scope restored after panic");
    }

    #[test]
    fn als_error_display_is_actionable() {
        let err = AlsError::NotSet;
        let msg = format!("{err}");
        assert!(
            msg.contains("not installed"),
            "the error message should hint at the fix: {msg}"
        );
    }
}
