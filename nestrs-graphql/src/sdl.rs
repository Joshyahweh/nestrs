//! Export schema as SDL for CI, federation gateways, and codegen.

pub use async_graphql::SDLExportOptions; // re-export from async_graphql root
use async_graphql::{ObjectType, Schema, SubscriptionType};

/// Full SDL for the executable schema (see also [`Schema::sdl_with_options`]).
pub fn export_schema_sdl<Q, M, S>(schema: &Schema<Q, M, S>) -> String
where
    Q: ObjectType + 'static,
    M: ObjectType + 'static,
    S: SubscriptionType + 'static,
{
    schema.sdl()
}

/// SDL export with [`SDLExportOptions`] (federation / formatting flags).
pub fn export_schema_sdl_with_options<Q, M, S>(
    schema: &Schema<Q, M, S>,
    options: SDLExportOptions,
) -> String
where
    Q: ObjectType + 'static,
    M: ObjectType + 'static,
    S: SubscriptionType + 'static,
{
    schema.sdl_with_options(options)
}
