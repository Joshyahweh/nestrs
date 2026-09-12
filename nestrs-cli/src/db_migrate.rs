//! `nestrs-cli db migrate` — add / run / revert / info.
//!
//! File convention: `<NNN>_<name>.sql` (single-shot) or
//! `<NNN>_<name>.up.sql` + `<NNN>_<name>.down.sql` (reversible). The
//! `NNN` is a zero-padded 3-digit sequence derived from the largest
//! existing migration in `--path` (default `./migrations/`), incremented
//! by one. Reversible flag toggles between the two shapes; sqlx's
//! migrator recognizes both.
//!
//! ## Why sequence-only, not timestamped
//!
//! sqlx-cli uses `<TIMESTAMP>_<rand>` for collision-safety in
//! parallel workflows. SeaORM uses `<TIMESTAMP>_<name>`. We use
//! `<NNN>_<name>` — no timestamp, no random suffix. Reasoning:
//! deterministic, greppable, sortable, and easy to write by hand if
//! someone migrates from another system. The risk of two contributors
//! picking the same NNN is low at our scale; if it happens, the
//! collision surfaces immediately at add-time (one would write 005,
//! the other 005; second writer sees 005 exists and walks to 006).
//! That's a fine workflow for the v0.5.x user base.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::db::{resolve_database_url, Backend, DbOptions};

pub(crate) fn run(args: &[String], opts: &DbOptions) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "missing `migrate` subcommand (expected `add`, `run`, `revert`, or `info`)".to_string(),
        );
    }
    match args[0].as_str() {
        "add" => add(&args[1..], opts),
        "run" => run_migrations(&args[1..], opts, MigrateDirection::Up),
        "revert" => run_migrations(&args[1..], opts, MigrateDirection::Down),
        "info" => info(opts),
        other => Err(format!(
            "unknown `migrate` subcommand `{other}` (expected `add`, `run`, `revert`, or `info`)"
        )),
    }
}

#[derive(Debug, Clone, Copy)]
enum MigrateDirection {
    Up,
    Down,
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

fn add(args: &[String], opts: &DbOptions) -> Result<(), String> {
    if opts.backend == Backend::Prisma {
        return add_prisma(args);
    }

    if args.is_empty() {
        return Err("expected `db migrate add <name> [--reversible]`".to_string());
    }
    let name = &args[0];
    validate_migration_name(name)?;
    let reversible = args.iter().any(|a| a == "--reversible");

    fs::create_dir_all(&opts.migrations_path).map_err(|e| format!("create migrations dir: {e}"))?;

    let next_n = next_sequence_number(&opts.migrations_path)?;
    let prefix = format!("{next_n:03}_{name}");

    if reversible {
        let up = opts.migrations_path.join(format!("{prefix}.up.sql"));
        let down = opts.migrations_path.join(format!("{prefix}.down.sql"));
        write_empty_template(&up, "-- up migration\n")?;
        write_empty_template(&down, "-- down migration\n")?;
        println!("created {} and {}", up.display(), down.display());
    } else {
        let f = opts.migrations_path.join(format!("{prefix}.sql"));
        write_empty_template(&f, "-- migration\n")?;
        println!("created {}", f.display());
    }
    Ok(())
}

fn add_prisma(args: &[String]) -> Result<(), String> {
    // `nestrs-cli db --backend prisma migrate add <name>` → `npx prisma migrate dev --name <name> --create-only`.
    // `--create-only` matches the `add` semantics: write a migration
    // file but don't apply it. `npx` is what `nestrs-prisma` already
    // uses internally (`nestrs-prisma/src/lib.rs:81`).
    if args.is_empty() {
        return Err("expected `db --backend prisma migrate add <name>`".to_string());
    }
    let name = &args[0];
    validate_migration_name(name)?;
    let status = Command::new("npx")
        .args(["prisma", "migrate", "dev", "--name", name, "--create-only"])
        .status()
        .map_err(|e| {
            format!("failed to spawn `npx prisma migrate dev` (is `npx` on PATH?): {e}")
        })?;
    if !status.success() {
        return Err(format!(
            "`npx prisma migrate dev --create-only` exited with status {status}"
        ));
    }
    Ok(())
}

fn validate_migration_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("migration name must not be empty".to_string());
    }
    if name.len() > 64 {
        return Err("migration name must be ≤ 64 characters".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(
            "migration name must match `[a-z0-9_]+` (lowercase, digits, underscores only)"
                .to_string(),
        );
    }
    Ok(())
}

fn write_empty_template(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents).map_err(|e| format!("write {}: {e}", path.display()))
}

fn next_sequence_number(dir: &Path) -> Result<u32, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(1),
        Err(e) => return Err(format!("read migrations dir: {e}")),
    };
    let mut max_n: u32 = 0;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // Match `NNN_*` (3+ digits, underscore, then anything).
        let Some((prefix, _)) = name_str.split_once('_') else {
            continue;
        };
        if let Ok(n) = prefix.parse::<u32>() {
            if n > max_n {
                max_n = n;
            }
        }
    }
    Ok(max_n + 1)
}

// ---------------------------------------------------------------------------
// run / revert
// ---------------------------------------------------------------------------

fn run_migrations(
    _args: &[String],
    opts: &DbOptions,
    direction: MigrateDirection,
) -> Result<(), String> {
    if opts.backend == Backend::Prisma {
        return prisma_pass_through(opts, direction);
    }
    run_sqlx(opts, direction)
}

fn run_sqlx(opts: &DbOptions, direction: MigrateDirection) -> Result<(), String> {
    // sqlx::any requires an explicit `install_default_drivers()` call
    // before the first connection. Without it, `AnyPool::connect`
    // panics with "No drivers installed" at the first query. The call
    // is idempotent — sqlx uses an `OnceLock`-style registry internally.
    sqlx::any::install_default_drivers();
    let url = resolve_database_url(opts)?;
    if !opts.migrations_path.is_dir() {
        return Err(format!(
            "migrations path does not exist or is not a directory: {}",
            opts.migrations_path.display()
        ));
    }
    // sqlx 0.8's `Migrator::new` takes `PathBuf` by value (impl
    // `MigrationSource<'static> for PathBuf`); we clone here so the
    // original `opts` is still usable in the `info` handler later.
    let path = opts.migrations_path.clone();
    let target_version = opts.target_version;
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("create tokio runtime: {e}"))?;
    rt.block_on(async move {
        use sqlx::migrate::MigrateError;
        let migrator = match sqlx::migrate::Migrator::new(path).await {
            Ok(m) => m,
            Err(MigrateError::VersionMissing(_)) | Err(MigrateError::VersionMismatch(_)) => {
                return Err(
                    "migrations are not sqlx-compatible (missing or wrong `_sqlx_migrations` tracking)".to_string(),
                );
            }
            Err(e) => return Err(format!("load migrator: {e}")),
        };
        let pool = match sqlx::AnyPool::connect(&url).await {
            Ok(p) => p,
            Err(e) => return Err(format!("connect `{url}`: {e}")),
        };
        // sqlx 0.8 `Migrator::undo` requires a target version — sqlx
        // does not have a "revert the most recent one" shorthand
        // because that would be ambiguous when several reversible
        // migrations are pending. We default to "undo to one before
        // the most recent applied" when no --target-version is given:
        // query `_sqlx_migrations` for the max version, subtract 1.
        let result = match direction {
            MigrateDirection::Up => migrator.run(&pool).await,
            MigrateDirection::Down => {
                let target = match target_version {
                    Some(v) => v,
                    None => match most_recent_applied_version(&pool).await {
                        Ok(v) => v.saturating_sub(1),
                        Err(e) => return Err(e),
                    },
                };
                migrator.undo(&pool, target).await
            }
        };
        match result {
            Ok(()) => {
                let verb = match direction {
                    MigrateDirection::Up => "applied",
                    MigrateDirection::Down => "reverted",
                };
                println!("migrations {verb}");
                Ok(())
            }
            Err(e) => Err(format!("migration {} failed: {e}", match direction {
                MigrateDirection::Up => "apply",
                MigrateDirection::Down => "revert",
            })),
        }
    })
}

async fn most_recent_applied_version(pool: &sqlx::AnyPool) -> Result<i64, String> {
    let (max_version,): (Option<i64>,) =
        sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(pool)
            .await
            .map_err(|e| format!("query _sqlx_migrations for max version: {e}"))?;
    Ok(max_version.unwrap_or(0))
}

fn prisma_pass_through(opts: &DbOptions, direction: MigrateDirection) -> Result<(), String> {
    let url = resolve_database_url(opts)?;
    let npx_args: Vec<&str> = match direction {
        MigrateDirection::Up => {
            // `npx prisma migrate deploy` is what
            // `nestrs-prisma::prisma_migrate_deploy_command()` returns.
            // Pass DATABASE_URL through so users don't have to remember
            // to set it for the sub-process.
            vec!["prisma", "migrate", "deploy"]
        }
        MigrateDirection::Down => {
            // Prisma's CLI doesn't have a first-class "revert" — the
            // closest documented flow is `migrate resolve --rolled-back
            // <version>` to mark a migration as rolled back. For
            // actual revert, users write a new migration that undoes
            // the change. We mirror that limitation by emitting the
            // resolve command, which lets the user mark the migration
            // failed.
            return Err(
                "Prisma does not support a first-class revert; use `npx prisma migrate resolve --rolled-back <version>` manually".to_string(),
            );
        }
    };
    let status = Command::new("npx")
        .args(&npx_args)
        .env("DATABASE_URL", &url)
        .status()
        .map_err(|e| format!("spawn `npx prisma ...`: {e}"))?;
    if !status.success() {
        return Err(format!(
            "prisma migrate pass-through exited with status {status}"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// info
// ---------------------------------------------------------------------------

fn info(opts: &DbOptions) -> Result<(), String> {
    if opts.backend == Backend::Prisma {
        // Pass-through to `npx prisma migrate status`. We don't try
        // to parse Prisma's output — just forward.
        let status = Command::new("npx")
            .args(["prisma", "migrate", "status"])
            .status()
            .map_err(|e| format!("spawn `npx prisma migrate status`: {e}"))?;
        if !status.success() {
            return Err(format!("prisma migrate status exited {status}"));
        }
        return Ok(());
    }
    let url = resolve_database_url(opts)?;
    sqlx::any::install_default_drivers();
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("create tokio runtime: {e}"))?;
    rt.block_on(async move {
        let pool = match sqlx::AnyPool::connect(&url).await {
            Ok(p) => p,
            Err(e) => return Err(format!("connect `{url}`: {e}")),
        };
        // sqlx exposes the applied-migration set via `list` / a query
        // against `_sqlx_migrations`. We do the query ourselves to
        // give a uniform output regardless of sqlx minor version.
        let applied: Vec<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
            "SELECT version, description FROM _sqlx_migrations ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .map_err(|e| format!("query _sqlx_migrations: {e}"))?;
        let files = list_migration_files(&opts.migrations_path)?;
        let applied_versions: std::collections::HashSet<i64> =
            applied.iter().map(|(v, _)| *v).collect();
        println!("Applied (from {}):", opts.migrations_path.display());
        if applied.is_empty() {
            println!("  (none)");
        }
        for (v, desc) in &applied {
            let marker = if files.iter().any(|f| f.0 == *v) {
                ""
            } else {
                " [missing on disk]"
            };
            println!("  {v:>3} {desc}{marker}");
        }
        println!("Files ({}):", opts.migrations_path.display());
        let mut pending = 0;
        for (v, file) in &files {
            let marker = if applied_versions.contains(v) {
                "applied"
            } else {
                pending += 1;
                "pending"
            };
            println!("  {v:>3} {file} [{marker}]");
        }
        println!("Total: {} applied, {} pending", applied.len(), pending);
        Ok(())
    })
}

fn list_migration_files(dir: &Path) -> Result<Vec<(i64, String)>, String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read migrations dir: {e}")),
    };
    let mut out: Vec<(i64, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("read dir entry: {e}"))?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // Single-shot: `NNN_<name>.sql`; reversible: `NNN_<name>.up.sql`.
        // Skip `.down.sql` — sqlx treats it as the inverse of `.up.sql`.
        if name_str.ends_with(".down.sql") {
            continue;
        }
        let stem = name_str
            .strip_suffix(".up.sql")
            .or_else(|| name_str.strip_suffix(".sql"))
            .unwrap_or(name_str);
        let Some((prefix, _rest)) = stem.split_once('_') else {
            continue;
        };
        if let Ok(v) = prefix.parse::<i64>() {
            out.push((v, name_str.to_string()));
        }
    }
    out.sort_by_key(|(v, _)| *v);
    Ok(out)
}
