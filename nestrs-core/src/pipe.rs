//! Pipes — transform / validate a single value before it reaches the handler (NestJS `PipeTransform`).

/// Transform one value into another, possibly failing (validation / coercion).
///
/// Use from handlers by calling [`PipeTransform::transform`] on a unit struct (or stateful pipe
/// type registered in DI). Route-level `#[use_pipes]` integration is not required for this trait to
/// be useful.
#[async_trait::async_trait]
pub trait PipeTransform<Input>: Send + Sync {
    type Output;
    type Error;
    async fn transform(&self, value: Input) -> Result<Self::Output, Self::Error>;
}
