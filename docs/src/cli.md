# CLI (`nestrs` / `nestrs-scaffold`)

The **nestrs** command-line tool scaffolds projects and source files. On [crates.io](https://crates.io/crates/nestrs-scaffold) the package is published as **`nestrs-scaffold`** (the `nestrs-cli` name is taken); the binary you run is still **`nestrs`**.

```bash
cargo install nestrs-scaffold
nestrs --help
```

## Commands (overview)

| Command | Purpose |
|---------|---------|
| `nestrs new <name>` | Create a new single-crate app with `Cargo.toml`, `src/main.rs`, starter module/controller, `.env.example`, optional git init. |
| `nestrs generate …` / `nestrs g …` | Generate **resource** (multi-transport scaffold), **service**, **controller**, **module**, **dto**, **guard**, **pipe**, **filter**, **interceptor**, **strategy**, **resolver**, **gateway**, **microservice**, **transport**, or bulk **resources**. |
| `nestrs doctor` | Print `rustc` / `cargo` versions, scan `Cargo.toml` for `nestrs` feature hints, and heuristically check `src/**/*.rs` for common misconfigurations (e.g. `enable_openapi()` without the `openapi` feature). Does not replace `cargo check`. |

### Running from a nestrs git clone (`cargo nestrs`)

The workspace defines a Cargo alias so you can invoke the CLI without a global install:

```bash
cargo nestrs doctor
cargo nestrs generate resource items --transport rest --path src
```

This is implemented in [`.cargo/config.toml`](../../.cargo/config.toml) as `run -p nestrs-scaffold --bin nestrs --`.

Short aliases for `generate`: `g`, and for kinds (`res`, `co`, `mo`, `dto`, …) — see `nestrs g --help` in your install.

Flags common to generators: `--style nest|rust` (file layout), `--path <dir>`, `--dry-run`, `--force`, `--quiet`. `g resource` also supports `--transport rest|graphql|ws|grpc|microservice` and `--no-interactive`.

## Scope vs Nest CLI

Nest’s CLI covers **application lifecycle**, **monorepos**, **libraries**, **plugins**, and **npm-style scripts**. nestrs intentionally stays closer to **Cargo** and normal Rust workflows.

| Nest CLI area | nestrs | What to use instead |
|---------------|--------|---------------------|
| `nest new` / app skeleton | **Yes (partial)** | `nestrs new` — single crate, opinionated starter, not every Nest default file. |
| `nest generate` (CRUD, modules, …) | **Yes (partial)** | `nestrs generate` — overlapping generators; naming and file trees differ; see templates in repo. |
| **Workspaces** (multiple apps/libs in one repo) | **No** | [Cargo workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html): one root `Cargo.toml` with `[workspace].members`, `cargo new` / `cargo new --lib` per crate, path dependencies. |
| **Libraries** (publishable `@nestjs/...`-style packages) | **No** | `cargo new --lib`, workspace members, versioned crates published to crates.io, or path/git deps — same as any Rust library. |
| **Scripts** (`npm run …` in `package.json`) | **No** | `cargo run --bin <name>`, [cargo-make](https://github.com/sagiegurari/cargo-make), [just](https://github.com/casey/just), Make, or shell scripts committed beside the repo. |
| `nest build` / `nest start` | **No** | `cargo build`, `cargo run`; production: binary + process manager or container. |
| Plugins extending the CLI | **No** | Fork or wrap `nestrs-scaffold`, or generate with your own templates. |

This keeps **one** Rust toolchain story (Cargo) instead of duplicating workspace and script features Nest layers on Node.

## Package manager note

`nestrs new` accepts `--package-manager cargo` (default). Other package managers are not supported — there is no parallel to Nest’s npm/yarn/pnpm integration.

## Related

- Crate README: [`nestrs-cli/README.md`](../../nestrs-cli/README.md) in the repo (source of `nestrs-scaffold`).
- Application structure and modules: [Introduction](index.md) and [Ecosystem modules](ecosystem.md).
