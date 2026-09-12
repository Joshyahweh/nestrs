# CLI (`nestrs-cli` / `nestrs-scaffold`)

The **nestrs-cli** command-line tool scaffolds projects and source files. On [crates.io](https://crates.io/crates/nestrs-scaffold) the package is published as **`nestrs-scaffold`** (the `nestrs-cli` name is taken); the binary you run is **`nestrs-cli`**.

**Command examples:** See the [API cookbook](appendix-api-cookbook.md) for additional **`nestrs-cli g …`** invocations alongside the **Examples** section below.

```bash
cargo install nestrs-scaffold
nestrs-cli --help
```

## Commands (overview)

| Command | Purpose |
|---------|---------|
| `nestrs-cli new <name>` | Create a new single-crate app with `Cargo.toml`, `src/main.rs`, starter module/controller, `.env.example`, optional git init. |
| `nestrs-cli generate …` / `nestrs-cli g …` | Generate **resource** (multi-transport scaffold), **service**, **controller**, **module**, **dto**, **guard**, **pipe**, **filter**, **interceptor**, **strategy**, **resolver**, **gateway**, **microservice**, **transport**, or bulk **resources**. |
| `nestrs-cli db migrate …` | Create, apply, revert, and inspect SQL migrations (`add`, `run`, `revert`, `info`). Default backend `sqlx`; `--backend prisma` passes through to `npx prisma`. |
| `nestrs-cli db seed …` | Seed the database from a Cargo seed binary (`--bin`) or a transactional SQL file (`--seed-file`). |
| `nestrs-cli doctor` | Print `rustc` / `cargo` versions, scan `Cargo.toml` for `nestrs` feature hints, and heuristically check `src/**/*.rs` for common misconfigurations (e.g. `enable_openapi()` without the `openapi` feature). Does not replace `cargo check`. |

The `db` family ships behind the `db` Cargo feature (it pulls in `sqlx` and `tokio` as CLI dependencies):

```bash
cargo install nestrs-scaffold --features db
```

## db — migrations and seeding

`nestrs-cli db` is the migrations + seeding runner. It works on plain SQL migration files — no DDL-diff engine (`synchronize`) or entity auto-discovery; those belong to full ORMs.

```text
nestrs-cli db [--backend sqlx|prisma] migrate add <name> [--reversible] [--path <dir>]
nestrs-cli db [--backend sqlx|prisma] migrate run [--path <dir>] [--target-version V] [--database-url URL]
nestrs-cli db [--backend sqlx|prisma] migrate revert [--path <dir>] [--target-version V] [--database-url URL]
nestrs-cli db [--backend sqlx|prisma] migrate info [--path <dir>] [--database-url URL]
nestrs-cli db [--backend sqlx|prisma] seed --bin <name> [--manifest-path <path>] [--database-url URL]
nestrs-cli db [--backend sqlx|prisma] seed --seed-file <path> [--database-url URL]
```

**Global flags:** `--backend sqlx|prisma` (default `sqlx`; `prisma` shells out to `npx prisma …` and cannot revert), `--path <dir>` (default `./migrations`), `--database-url URL` (resolution order: `--database-url` > `DATABASE_URL` > `NESTRS_DB__URL`), `--target-version V`.

**migrate add** writes empty SQL files with sequence-numbered names: `<NNN>_<name>.sql`, or `<NNN>_<name>.up.sql` + `<NNN>_<name>.down.sql` with `--reversible`. `NNN` is a zero-padded 3-digit sequence derived from the largest existing migration (deterministic and hand-writable, unlike timestamped schemes).

**migrate run / revert / info** apply pending migrations up, walk down (`--target-version 0` = full revert), and list applied/pending state. The sqlx backend uses `sqlx::migrate::Migrator` (`_sqlx_migrations` records history).

**seed** has two modes: `--bin <name>` passes through to `cargo run --bin <name>`, forwarding the resolved URL as both `DATABASE_URL` and `NESTRS_DB__URL` (you own the seed binary — the common case composes with `CrudService<T>`); `--seed-file <path>` executes a SQL file via `sqlx::raw_sql` inside an explicit transaction, so a mid-file failure rolls back earlier rows (the `pg_dump` escape hatch).

See [CRUD generation](crud.md) for `#[crud]` controllers over the same pool, and [Ecosystem modules](ecosystem.md) for the database modules.

### Running from a nestrs git clone (`cargo nestrs`)

The workspace defines a Cargo alias so you can invoke the CLI without a global install:

```bash
cargo nestrs doctor
cargo nestrs generate resource items --transport rest --path src
```

This is implemented in [`.cargo/config.toml`](../../.cargo/config.toml) as `run -p nestrs-scaffold --bin nestrs-cli --`.

Short aliases for `generate`: `g`, and for kinds (`res`, `co`, `mo`, `dto`, …) — see `nestrs-cli g --help` in your install.

Flags common to generators: `--style nest|rust` (file layout), `--path <dir>`, `--dry-run`, `--force`, `--quiet`. `g resource` also supports `--transport rest|graphql|ws|grpc|microservice` and `--no-interactive`.

## Scope vs Nest CLI

Nest’s CLI covers **application lifecycle**, **monorepos**, **libraries**, **plugins**, and **npm-style scripts**. nestrs intentionally stays closer to **Cargo** and normal Rust workflows.

| Nest CLI area | nestrs | What to use instead |
|---------------|--------|---------------------|
| `nest new` / app skeleton | **Yes (partial)** | `nestrs-cli new` — single crate, opinionated starter, not every Nest default file. |
| `nest generate` (CRUD, modules, …) | **Yes (partial)** | `nestrs-cli generate` — overlapping generators; naming and file trees differ; see templates in repo. |
| **Workspaces** (multiple apps/libs in one repo) | **No** | [Cargo workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html): one root `Cargo.toml` with `[workspace].members`, `cargo new` / `cargo new --lib` per crate, path dependencies. |
| **Libraries** (publishable `@nestjs/...`-style packages) | **No** | `cargo new --lib`, workspace members, versioned crates published to crates.io, or path/git deps — same as any Rust library. |
| **Scripts** (`npm run …` in `package.json`) | **No** | `cargo run --bin <name>`, [cargo-make](https://github.com/sagiegurari/cargo-make), [just](https://github.com/casey/just), Make, or shell scripts committed beside the repo. |
| `nest build` / `nest start` | **No** | `cargo build`, `cargo run`; production: binary + process manager or container. |
| Plugins extending the CLI | **No** | Fork or wrap `nestrs-scaffold`, or generate with your own templates. |

This keeps **one** Rust toolchain story (Cargo) instead of duplicating workspace and script features Nest layers on Node.

## Package manager note

`nestrs-cli new` accepts `--package-manager cargo` (default). Other package managers are not supported — there is no parallel to Nest’s npm/yarn/pnpm integration.

## Examples

**New project** (interactive prompts may vary by version):

```bash
cargo install nestrs-scaffold
nestrs-cli new billing-api
cd billing-api
cargo run
```

**Generate a REST-shaped resource** into `src/` (dry run first in unfamiliar trees):

```bash
nestrs-cli generate resource invoices --transport rest --path src --dry-run
nestrs-cli generate resource invoices --transport rest --path src
```

**Workspace doctor** (from a clone of nestrs):

```bash
cargo nestrs doctor
```

Use `doctor` output as a **hint list**: it catches common feature/Cargo mismatches but does not replace `cargo check` or your CI matrix.

## Related

- Crate README: [`nestrs-cli/README.md`](../../nestrs-cli/README.md) in the repo (source of `nestrs-scaffold`).
- Application structure and modules: [Introduction](index.md) and [Ecosystem modules](ecosystem.md).
