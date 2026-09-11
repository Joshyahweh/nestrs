// Every test here drives `nestrs-cli db …`, which the binary gates on its
// own `db` Cargo feature (src/main.rs `match` arm). Under plain
// `cargo test --workspace` no workspace member enables that feature, so the
// spawned binary would reject every invocation; the tests are opt-in just
// like the subcommand. CI runs with `--all-features`.
#![cfg(feature = "db")]

//! Wave 4.3 — `nestrs-cli db` CLI integration tests.
//!
//! 12 tests covering: migration add (with sequence + reversible +
//! validation), migration run/revert on SQLite, info listing,
//! seed `--bin` pass-through (exit code + env forwarding),
//! seed `--seed-file` (apply + transactional rollback on error).
//!
//! SQLite is the only driver exercised; Postgres parity is documented
//! as a follow-up (the migration code path is sqlx 0.8's `Migrator`,
//! which uses the same Any-driver surface, but driver-specific quirks
//! like quote escaping in `_sqlx_migrations` aren't covered here).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Test helpers — mirror `tests/generator_cli.rs`
// ---------------------------------------------------------------------------

fn cli_bin() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_nestrs-cli") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_nestrs_cli") {
        return PathBuf::from(p);
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let crate_dir = PathBuf::from(manifest);
    let workspace_root = crate_dir
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists() && p.join("nestrs-cli").exists())
        .or_else(|| crate_dir.ancestors().nth(1))
        .unwrap_or(&crate_dir)
        .to_path_buf();
    let bin_name = if cfg!(windows) {
        "nestrs-cli.exe"
    } else {
        "nestrs-cli"
    };
    for candidate in [
        workspace_root.join("target").join("debug").join(bin_name),
        workspace_root.join("target").join("debug").join("deps").join(bin_name),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!(
        "could not locate nestrs-cli; CARGO_BIN_EXE_* not set and binary not in target/debug"
    );
}

fn unique_tmp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("nestrs-cli-db-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

fn sqlite_url(file: &Path) -> String {
    // sqlx 0.8 requires explicit `mode=rwc` for SQLite file URLs we
    // want to write to. Mirrors the `examples/hello-app/src/main.rs:177`
    // pattern.
    format!("sqlite://{}?mode=rwc", file.display())
}

/// Run the CLI with `cwd` set to `dir`, and return the resulting
/// `Output` so the test can inspect both status and stdout/stderr.
fn run_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(cli_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn nestrs-cli")
}

fn assert_success(out: &std::process::Output, label: &str) {
    assert!(
        out.status.success(),
        "{label}: cli exited non-zero (status={:?})\nstderr: {}\nstdout: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout),
    );
}

fn assert_failure(out: &std::process::Output, label: &str) {
    assert!(
        !out.status.success(),
        "{label}: cli unexpectedly succeeded\nstdout: {}",
        String::from_utf8_lossy(&out.stdout),
    );
}

// ---------------------------------------------------------------------------
// Tests 1–4: `db migrate add`
// ---------------------------------------------------------------------------

#[test]
fn migrate_add_creates_version_stamped_file() {
    let dir = unique_tmp_dir("add-empty");
    let out = run_in(
        &dir,
        &["db", "migrate", "add", "create_users", "--path", "migrations"],
    );
    assert_success(&out, "migrate add create_users");
    let f = dir.join("migrations").join("001_create_users.sql");
    assert!(f.exists(), "expected {} to exist", f.display());
}

#[test]
fn migrate_add_with_reversible_creates_up_and_down() {
    let dir = unique_tmp_dir("add-rev");
    let out = run_in(
        &dir,
        &[
            "db",
            "migrate",
            "add",
            "create_posts",
            "--reversible",
            "--path",
            "migrations",
        ],
    );
    assert_success(&out, "migrate add create_posts --reversible");
    let up = dir.join("migrations").join("001_create_posts.up.sql");
    let down = dir.join("migrations").join("001_create_posts.down.sql");
    assert!(up.exists(), "expected {}", up.display());
    assert!(down.exists(), "expected {}", down.display());
}

#[test]
fn migrate_add_increments_existing_sequence() {
    let dir = unique_tmp_dir("add-seq");
    fs::create_dir_all(dir.join("migrations")).unwrap();
    // Pre-create a higher-NNN file to verify the next call walks the
    // sequence rather than starting at 001.
    fs::write(dir.join("migrations").join("003_legacy.sql"), "-- legacy\n").unwrap();
    let out = run_in(
        &dir,
        &["db", "migrate", "add", "new_thing", "--path", "migrations"],
    );
    assert_success(&out, "migrate add new_thing");
    let f = dir.join("migrations").join("004_new_thing.sql");
    assert!(
        f.exists(),
        "expected {} to exist; got: {:?}",
        f.display(),
        fs::read_dir(dir.join("migrations"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect::<Vec<_>>()
    );
}

#[test]
fn migrate_add_rejects_invalid_name() {
    let dir = unique_tmp_dir("add-bad");
    // Names with `/` are filesystem-hostile. Names with uppercase or
    // hyphens are not part of `[a-z0-9_]+`. Empty name is rejected at
    // validation time.
    for bad in ["BadName", "has-dash", "has space", "", "with/slash"] {
        let out = run_in(
            &dir,
            &["db", "migrate", "add", bad, "--path", "migrations"],
        );
        assert_failure(
            &out,
            &format!("migrate add {bad:?} should fail (invalid name)"),
        );
    }
}

// ---------------------------------------------------------------------------
// Tests 5–8: `db migrate run / revert / info`
// ---------------------------------------------------------------------------

/// Write a pair of forward migrations to `migrations/` and apply
/// them. Returns the (sqlite url, migrations dir) the tests share.
fn two_migrations_applied(label: &str) -> (PathBuf, PathBuf) {
    let dir = unique_tmp_dir(label);
    fs::create_dir_all(dir.join("migrations")).unwrap();
    fs::write(
        dir.join("migrations").join("001_create_widgets.sql"),
        "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n\
         INSERT INTO widgets (name) VALUES ('a'), ('b');\n",
    )
    .unwrap();
    fs::write(
        dir.join("migrations").join("002_seed_widgets.sql"),
        "INSERT INTO widgets (name) VALUES ('c'), ('d');\n",
    )
    .unwrap();
    let db_file = dir.join("test.db");
    let url = sqlite_url(&db_file);
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "migrate run (setup)");
    (dir, db_file)
}

#[test]
fn migrate_run_applies_pending_in_order() {
    let (dir, db_file) = two_migrations_applied("run-applies");
    // After `migrate run`, opening the DB should show 4 rows: 2 from
    // migration 001, 2 from migration 002.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let count: (i64,) = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&sqlite_url(&db_file)).await.unwrap();
        sqlx::query_as("SELECT COUNT(*) FROM widgets")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(count.0, 4, "expected 4 rows after both migrations applied");
    // And `_sqlx_migrations` should have 2 rows.
    let applied: Vec<(i64,)> = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&sqlite_url(&db_file)).await.unwrap();
        sqlx::query_as("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap()
    });
    assert_eq!(applied.len(), 2);
    assert_eq!(applied[0].0, 1);
    assert_eq!(applied[1].0, 2);
    drop(dir);
}

#[test]
fn migrate_run_is_idempotent_on_already_applied_set() {
    let (dir, db_file) = two_migrations_applied("run-idempotent");
    let url = sqlite_url(&db_file);
    // Second run should be a no-op (no error, no rows added).
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "second migrate run");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let count: (i64,) = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query_as("SELECT COUNT(*) FROM widgets")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(count.0, 4, "second run must not duplicate rows");
}

#[test]
fn migrate_revert_on_reversible_migrations() {
    let dir = unique_tmp_dir("revert-rev");
    fs::create_dir_all(dir.join("migrations")).unwrap();
    fs::write(
        dir.join("migrations").join("001_create_gadgets.up.sql"),
        "CREATE TABLE gadgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n\
         INSERT INTO gadgets (name) VALUES ('g1');\n",
    )
    .unwrap();
    fs::write(
        dir.join("migrations").join("001_create_gadgets.down.sql"),
        "DROP TABLE gadgets;\n",
    )
    .unwrap();
    let db_file = dir.join("gadgets.db");
    let url = sqlite_url(&db_file);
    // Apply the reversible migration.
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "reversible run");
    // Verify table exists.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let exists: (i64,) = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='gadgets'")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(exists.0, 1, "gadgets table should exist after apply");
    // Revert (target_version defaults to max - 1; here, max is 1, so target is 0).
    let out = run_in(
        &dir,
        &["db", "migrate", "revert", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "reversible revert");
    let exists: (i64,) = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='gadgets'")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(exists.0, 0, "gadgets table should be gone after revert");
}

#[test]
fn migrate_info_lists_applied_and_pending() {
    let dir = unique_tmp_dir("info-list");
    fs::create_dir_all(dir.join("migrations")).unwrap();
    // Apply only 001 first (002 doesn't exist yet).
    fs::write(
        dir.join("migrations").join("001_one.sql"),
        "CREATE TABLE t1 (id INTEGER);\n",
    )
    .unwrap();
    let db_file = dir.join("info.db");
    let url = sqlite_url(&db_file);
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "apply 001");
    // Now drop in 002 — sqlx tracks only what's been applied; 002 is
    // pending. `info` should show 1 applied + 1 pending.
    fs::write(
        dir.join("migrations").join("002_two.sql"),
        "CREATE TABLE t2 (id INTEGER);\n",
    )
    .unwrap();
    let out = run_in(
        &dir,
        &["db", "migrate", "info", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "info");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("001"), "stdout missing 001: {stdout}");
    assert!(stdout.contains("002"), "stdout missing 002: {stdout}");
    assert!(stdout.contains("applied"), "stdout missing 'applied': {stdout}");
    assert!(stdout.contains("pending"), "stdout missing 'pending': {stdout}");
    assert!(stdout.contains("1 pending"), "stdout missing count: {stdout}");
}

// ---------------------------------------------------------------------------
// Tests 9–10: `db seed --bin`
// ---------------------------------------------------------------------------

/// Build a tiny scratch cargo project with one binary whose `main`
/// either exits 0 or exits 1. Returns the project root. The build is
/// shared across both tests 9 and 10 (called once per test for
/// isolation — cargo's incremental cache makes this ~1s warm).
fn scaffold_seed_project(label: &str, exit_code: i32) -> PathBuf {
    let dir = unique_tmp_dir(label);
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"seed-scratch\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n\n[[bin]]\nname = \"seed-bin\"\npath = \"src/main.rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src").join("main.rs"),
        format!("fn main() {{\n    std::process::exit({exit_code});\n}}\n"),
    )
    .unwrap();
    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .status()
        .expect("spawn cargo build");
    assert!(
        status.success(),
        "cargo build of scratch seed project failed: {status:?}"
    );
    dir
}

#[test]
fn seed_bin_runs_user_binary_and_forwards_exit_code() {
    let ok_dir = scaffold_seed_project("seed-ok", 0);
    let out = run_in(
        &ok_dir,
        &[
            "db",
            "seed",
            "--bin",
            "seed-bin",
            "--manifest-path",
            ok_dir.join("Cargo.toml").to_str().unwrap(),
            "--database-url",
            "sqlite::memory:",
        ],
    );
    assert_success(&out, "seed --bin (exit 0)");

    let bad_dir = scaffold_seed_project("seed-bad", 1);
    let out = run_in(
        &bad_dir,
        &[
            "db",
            "seed",
            "--bin",
            "seed-bin",
            "--manifest-path",
            bad_dir.join("Cargo.toml").to_str().unwrap(),
            "--database-url",
            "sqlite::memory:",
        ],
    );
    assert_failure(&out, "seed --bin (exit 1) should fail");
}

#[test]
fn seed_bin_passes_database_url_via_env() {
    let dir = unique_tmp_dir("seed-env");
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"seed-env-bin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\n\n[[bin]]\nname = \"printenv\"\npath = \"src/main.rs\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src").join("main.rs"),
        "fn main() {\n\
         let url = std::env::var(\"DATABASE_URL\").unwrap_or_default();\n\
         let ns = std::env::var(\"NESTRS_DB__URL\").unwrap_or_default();\n\
         println!(\"DATABASE_URL={url}\");\n\
         println!(\"NESTRS_DB__URL={ns}\");\n\
         }\n",
    )
    .unwrap();
    let status = Command::new("cargo")
        .arg("build")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .status()
        .expect("cargo build");
    assert!(status.success(), "cargo build failed: {status:?}");

    let out = run_in(
        &dir,
        &[
            "db",
            "seed",
            "--bin",
            "printenv",
            "--manifest-path",
            dir.join("Cargo.toml").to_str().unwrap(),
            "--database-url",
            "sqlite:./flagged.db?mode=rwc",
        ],
    );
    assert_success(&out, "seed --bin printenv");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("DATABASE_URL=sqlite:./flagged.db?mode=rwc"),
        "stdout missing DATABASE_URL flag value: {stdout}"
    );
    assert!(
        stdout.contains("NESTRS_DB__URL=sqlite:./flagged.db?mode=rwc"),
        "stdout missing NESTRS_DB__URL flag value: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Tests 11–12: `db seed --seed-file`
// ---------------------------------------------------------------------------

#[test]
fn seed_file_executes_sql_statements() {
    let dir = unique_tmp_dir("seed-file-ok");
    fs::create_dir_all(dir.join("migrations")).unwrap();
    fs::write(
        dir.join("migrations").join("001_create_things.sql"),
        "CREATE TABLE things (id INTEGER PRIMARY KEY, label TEXT NOT NULL);\n",
    )
    .unwrap();
    let db_file = dir.join("things.db");
    let url = sqlite_url(&db_file);
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "setup migrate run");
    // Write the seed file with two INSERTs.
    fs::write(
        dir.join("seeds.sql"),
        "INSERT INTO things (label) VALUES ('alpha');\n\
         INSERT INTO things (label) VALUES ('beta');\n",
    )
    .unwrap();
    let out = run_in(
        &dir,
        &["db", "seed", "--seed-file", "seeds.sql", "--database-url", &url],
    );
    assert_success(&out, "seed --seed-file");
    // Verify rows.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let rows: Vec<(String,)> = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query_as("SELECT label FROM things ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap()
    });
    assert_eq!(rows, vec![("alpha".to_string(),), ("beta".to_string(),)]);
}

#[test]
fn seed_file_rolls_back_on_sql_error() {
    let dir = unique_tmp_dir("seed-file-rollback");
    fs::create_dir_all(dir.join("migrations")).unwrap();
    fs::write(
        dir.join("migrations").join("001_create_fail.sql"),
        "CREATE TABLE fail_demo (id INTEGER PRIMARY KEY, label TEXT NOT NULL);\n",
    )
    .unwrap();
    let db_file = dir.join("fail.db");
    let url = sqlite_url(&db_file);
    let out = run_in(
        &dir,
        &["db", "migrate", "run", "--path", "migrations", "--database-url", &url],
    );
    assert_success(&out, "setup migrate run");
    // First INSERT is valid; second references a nonexistent table, which
    // SQLite will reject. The whole transaction must roll back so the
    // first INSERT is also undone.
    fs::write(
        dir.join("seed_fail.sql"),
        "INSERT INTO fail_demo (label) VALUES ('first');\n\
         INSERT INTO no_such_table (label) VALUES ('second');\n",
    )
    .unwrap();
    let out = run_in(
        &dir,
        &["db", "seed", "--seed-file", "seed_fail.sql", "--database-url", &url],
    );
    assert_failure(&out, "seed file with bad statement must fail");
    // Confirm the first INSERT was rolled back: table should exist
    // (DDL isn't transactional in SQLite but the INSERT is), but rows
    // count must be zero.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let count: (i64,) = rt.block_on(async {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query_as("SELECT COUNT(*) FROM fail_demo")
            .fetch_one(&pool)
            .await
            .unwrap()
    });
    assert_eq!(count.0, 0, "expected zero rows after rollback");
}
