//! `nestrs-cli db` — DB migrations + seeding CLI.
//!
//! Default-OFF behind the `db` Cargo feature (see `Cargo.toml`). When
//! the feature is off, `nestrs-cli db ...` errors with a clear message
//! instead of producing a half-built binary.
//!
//! ## Subcommand family
//!
//! ```text
//! nestrs-cli db [--backend sqlx|prisma] migrate add <name> [--reversible] [--path <dir>]
//! nestrs-cli db [--backend sqlx|prisma] migrate run [--path <dir>] [--target-version V] [--database-url URL]
//! nestrs-cli db [--backend sqlx|prisma] migrate revert [--path <dir>] [--target-version V] [--database-url URL]
//! nestrs-cli db [--backend sqlx|prisma] migrate info [--path <dir>] [--database-url URL]
//! nestrs-cli db [--backend sqlx|prisma] seed --bin <name> [--manifest-path <path>] [--database-url URL]
//! nestrs-cli db [--backend sqlx|prisma] seed --seed-file <path> [--database-url URL]
//! ```
//!
//! Default backend is `sqlx`. The Prisma backend is a pass-through to
//! `npx prisma ...` — the file-naming convention and migration
//! recording belong to Prisma; we just wire it up so a single CLI
//! command covers both backends.
//!
//! ## What this is — and isn't
//!
//! This is a runner, not a compiler. `db migrate add` writes
//! empty SQL files; the user fills them in. `db seed --bin` shells
//! out to `cargo run --bin <name>`; the user owns the seed binary.
//! `db seed --seed-file` runs a SQL file via `sqlx::raw_sql` inside
//! a transaction. There is no `synchronize` (no full DDL diff
//! engine), no in-memory schema migration, no auto-discovery of
//! entity definitions — those are TypeORM/MikroORM/SeaORM features
//! that ship in their respective ORMs and would be a multi-month
//! port here. Out of scope for this wave.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    Sqlx,
    Prisma,
}

#[derive(Debug, Clone)]
pub(crate) struct DbOptions {
    pub backend: Backend,
    pub migrations_path: PathBuf,
    pub database_url: Option<String>,
    pub target_version: Option<i64>,
}

/// Dispatcher: parse global flags, then hand off to `db_migrate` or
/// `db_seed`. Returns Err with a user-facing message on unknown
/// subcommands or missing args.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(missing_subcommand_help());
    }

    // Pull `--backend`, `--path`, `--database-url`, `--target-version`
    // out of the front of `args` in any order. Everything else is
    // passed to the subcommand handler.
    let mut opts = DbOptions {
        backend: Backend::Sqlx,
        migrations_path: PathBuf::from("migrations"),
        database_url: None,
        target_version: None,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --backend".to_string())?;
                opts.backend = match v.as_str() {
                    "sqlx" => Backend::Sqlx,
                    "prisma" => Backend::Prisma,
                    other => {
                        return Err(format!(
                            "unknown backend `{other}` (expected `sqlx` or `prisma`)"
                        ))
                    }
                };
            }
            "--path" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --path".to_string())?;
                opts.migrations_path = PathBuf::from(v);
            }
            "--database-url" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --database-url".to_string())?;
                opts.database_url = Some(v.clone());
            }
            "--target-version" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --target-version".to_string())?;
                opts.target_version = Some(
                    v.parse::<i64>()
                        .map_err(|e| format!("invalid --target-version `{v}`: {e}"))?,
                );
            }
            other => rest.push(other.to_string()),
        }
        i += 1;
    }

    if rest.is_empty() {
        return Err(missing_subcommand_help());
    }

    match rest[0].as_str() {
        "migrate" => crate::db_migrate::run(&rest[1..], &opts),
        "seed" => crate::db_seed::run(&rest[1..], &opts),
        other => Err(format!(
            "unknown `db` subcommand `{other}` (expected `migrate` or `seed`)"
        )),
    }
}

fn missing_subcommand_help() -> String {
    "missing `db` subcommand (expected `migrate` or `seed`); run `nestrs-cli --help` for usage"
        .to_string()
}

// Shared helpers used by `db_migrate` and `db_seed`. Kept `pub(crate)`
// so the modules can call them without round-tripping through `db::`.
pub(crate) fn resolve_database_url(opts: &DbOptions) -> Result<String, String> {
    if let Some(u) = opts.database_url.clone() {
        return Ok(u);
    }
    if let Ok(u) = std::env::var("DATABASE_URL") {
        if !u.is_empty() {
            return Ok(u);
        }
    }
    if let Ok(u) = std::env::var("NESTRS_DB__URL") {
        if !u.is_empty() {
            return Ok(u);
        }
    }
    Err("no database URL: pass --database-url, set DATABASE_URL, or set NESTRS_DB__URL".to_string())
}
