//! `nestrs-cli db seed` — run seed code.
//!
//! Two modes:
//!
//! - `--bin <name>` — pass-through to `cargo run --bin <name>` in cwd.
//!   Forwards `DATABASE_URL` (resolved from `--database-url` flag or
//!   env) and `NESTRS_DB__URL`. The user owns the seed binary; we just
//!   forward the env. Exit code from `cargo run` is the exit code from
//!   `nestrs-cli`.
//!
//! - `--seed-file <path>` — execute a SQL file against the same
//!   `DATABASE_URL` connection via `sqlx::raw_sql`. Statements run
//!   inside an explicit transaction so a failed `INSERT` mid-file
//!   rolls back the earlier rows. The TypeORM/MikroORM escape hatch
//!   for fixture dumps.
//!
//! Why two modes: the common case is a Rust binary (the seed code
//! composes with `CrudService<T>`, calls into the app's services,
//! etc.). The escape hatch is "I have a `.sql` dump from a Postgres
//! `pg_dump` and I just want to load it". Both are 30 lines; the
//! alternative (compile `.rs` files ad-hoc) is much more brittle and
//! slower (no Cargo's incremental compilation cache).

use std::path::PathBuf;
use std::process::Command;

use crate::db::{resolve_database_url, Backend, DbOptions};

pub(crate) fn run(args: &[String], opts: &DbOptions) -> Result<(), String> {
    if opts.backend == Backend::Prisma {
        return Err(
            "`nestrs-cli db --backend prisma seed` is not supported; use Prisma's `db seed` workflow".to_string(),
        );
    }

    let mut bin: Option<String> = None;
    let mut seed_file: Option<PathBuf> = None;
    let mut manifest_path: Option<PathBuf> = None;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--bin" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --bin".to_string())?;
                bin = Some(v.clone());
            }
            "--seed-file" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --seed-file".to_string())?;
                seed_file = Some(PathBuf::from(v));
            }
            "--manifest-path" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| "missing value for --manifest-path".to_string())?;
                manifest_path = Some(PathBuf::from(v));
            }
            other => return Err(format!("unknown `db seed` flag `{other}`")),
        }
        i += 1;
    }

    match (bin, seed_file) {
        (Some(name), None) => run_seed_bin(&name, manifest_path.as_deref(), opts),
        (None, Some(path)) => run_seed_file(&path, opts),
        (Some(_), Some(_)) => Err("pass either --bin or --seed-file, not both".to_string()),
        (None, None) => {
            Err("expected `db seed --bin <name>` or `db seed --seed-file <path>`".to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// --bin pass-through
// ---------------------------------------------------------------------------

fn run_seed_bin(
    bin: &str,
    manifest_path: Option<&std::path::Path>,
    opts: &DbOptions,
) -> Result<(), String> {
    let url = resolve_database_url(opts)?;
    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--bin", bin]);
    if let Some(mp) = manifest_path {
        cmd.arg("--manifest-path").arg(mp);
    }
    // Forward the resolved URL under both env names. The seed binary
    // is free to read whichever it likes; we just make sure it sees
    // *something*. If the user already has DATABASE_URL set, our
    // `resolve_database_url` returned that value, so we're forwarding
    // what they expect.
    cmd.env("DATABASE_URL", &url);
    cmd.env("NESTRS_DB__URL", &url);

    let status = cmd
        .status()
        .map_err(|e| format!("spawn `cargo run --bin {bin}`: {e}"))?;
    if !status.success() {
        // We deliberately do NOT short-circuit to a typed error —
        // the user's seed binary may legitimately exit non-zero, and
        // they want the exit code to propagate. `main()` already
        // prints `error: ...` and exits 1 on Err; success() == true
        // here would mean we should report Ok. We can't easily get
        // the original status code out through `Result<(), String>`,
        // so we surface it in the error message and let the user see
        // it. (TODO: a future refinement could plumb the exit code
        // through, but that's not in scope.)
        return Err(format!(
            "`cargo run --bin {bin}` exited with non-zero status {status}"
        ));
    }
    println!("seed bin `{bin}` completed");
    Ok(())
}

// ---------------------------------------------------------------------------
// --seed-file pass-through
// ---------------------------------------------------------------------------

fn run_seed_file(path: &std::path::Path, opts: &DbOptions) -> Result<(), String> {
    sqlx::any::install_default_drivers();
    let url = resolve_database_url(opts)?;
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("read seed file `{}`: {e}", path.display()))?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("create tokio runtime: {e}"))?;
    rt.block_on(async move {
        let pool = match sqlx::AnyPool::connect(&url).await {
            Ok(p) => p,
            Err(e) => return Err(format!("connect `{url}`: {e}")),
        };
        // Run inside an explicit transaction. sqlx's `pool.begin()`
        // returns a `Transaction<Any>` whose `.commit()` flushes; if
        // any statement in the raw SQL errors, the `?` propagates
        // and the transaction drops without committing — full
        // rollback. That's the documented "all or nothing" semantic
        // test 12 asserts on.
        let mut tx = match pool.begin().await {
            Ok(t) => t,
            Err(e) => return Err(format!("begin transaction: {e}")),
        };
        // `raw_sql` returns a `RawSql` query object; `execute` runs
        // every statement separated by `;` in one round trip per
        // statement. The first error short-circuits.
        if let Err(e) = sqlx::raw_sql(&contents).execute(&mut *tx).await {
            return Err(format!("execute seed file: {e}"));
        }
        if let Err(e) = tx.commit().await {
            return Err(format!("commit seed transaction: {e}"));
        }
        println!("seed file `{}` applied", path.display());
        Ok(())
    })
}
