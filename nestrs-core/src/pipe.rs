//! Pipes — transform / validate a single value before it reaches the handler (NestJS `PipeTransform`).
//!
//! ## Layers
//!
//! 1. [`PipeTransform`] — the base trait. Takes an `Input`, returns an `Output`,
//!    possibly failing. Generic over the input type so a single pipe can be
//!    reused across parameters.
//!
//! 2. [`HttpPipeTransform`] — a marker sub-trait that adds the `Default`
//!    bound the `#[use_pipes]` macro needs (so each macro-generated
//!    extractor can call `<P as Default>::default()` without DI). Mirrors
//!    the transport-specific sub-traits in `nestrs-ws` (`WsPipeTransform`)
//!    and `nestrs-microservices` (`MicroPipeTransform`).

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

/// Marker trait for pipe types usable in HTTP `#[use_pipes]`. Adds the
/// `Default` bound the macro needs to instantiate each pipe at extraction
/// time without going through DI, and constrains `Error` to
/// `std::error::Error` so the per-arity extractors can box pipe errors and
/// downcast to `HttpException` (preserving the per-pipe status code).
///
/// Implement this alongside your [`PipeTransform`] impl for each input
/// type your pipe accepts. The macro emits a per-arity extractor that
/// calls `<P as Default>::default().transform(value).await?` for each
/// pipe in declaration order.
pub trait HttpPipeTransform<Input>:
    PipeTransform<Input, Error: std::error::Error + Send + Sync> + Default + Send + Sync + 'static
{
}
