//! Export schema as SDL for CI, federation gateways, and codegen.

pub use async_graphql::SDLExportOptions; // re-export from async_graphql root
use async_graphql::{ObjectType, Schema, SubscriptionType};

use std::fs;
use std::io::Write;
use std::path::Path;

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

/// Wave 7.11 — write the full SDL of a schema to disk. Creates parent
/// directories if needed. Returns the number of bytes written.
pub fn export_sdl_to_file<Q, M, S>(
    schema: &Schema<Q, M, S>,
    path: impl AsRef<Path>,
) -> Result<usize, String>
where
    Q: ObjectType + 'static,
    M: ObjectType + 'static,
    S: SubscriptionType + 'static,
{
    write_sdl_to_file(&export_schema_sdl(schema), path)
}

/// Wave 7.11 — SDL export with [`SDLExportOptions`] written to disk.
/// Federation-flagged form, for handing a subgraph SDL to an Apollo
/// Router / GraphOS Studio.
pub fn export_sdl_with_options_to_file<Q, M, S>(
    schema: &Schema<Q, M, S>,
    options: SDLExportOptions,
    path: impl AsRef<Path>,
) -> Result<usize, String>
where
    Q: ObjectType + 'static,
    M: ObjectType + 'static,
    S: SubscriptionType + 'static,
{
    write_sdl_to_file(&export_schema_sdl_with_options(schema, options), path)
}

fn write_sdl_to_file(sdl: &str, path: impl AsRef<Path>) -> Result<usize, String> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut f = fs::File::create(path).map_err(|e| e.to_string())?;
    f.write_all(sdl.as_bytes()).map_err(|e| e.to_string())?;
    Ok(sdl.len())
}
