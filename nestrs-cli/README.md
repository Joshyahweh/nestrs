# nestrs-scaffold

**CLI scaffolding** for [nestrs](https://crates.io/crates/nestrs): generate modules, resources, controllers, DTOs, guards, transports, and more (Nest-style file layout or idiomatic Rust filenames).

The crates.io package name is **`nestrs-scaffold`** because the **`nestrs-cli`** name is taken. The installed binary is still **`nestrs`**.

**Docs:** [docs.rs/nestrs-scaffold](https://docs.rs/nestrs-scaffold) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs) · **MdBook:** `docs/src/cli.md` (see repo `docs/` + `SUMMARY.md`)

## Install

```bash
cargo install nestrs-scaffold
```

Verify:

```bash
nestrs --help
```

## Examples

Create a new project:

```bash
nestrs new my-api
```

Add a REST resource (from your crate root):

```bash
nestrs generate resource users
```

Other subcommands and flags are listed in `nestrs --help` / `nestrs generate --help`.

## Scope vs Nest CLI

| Nest CLI | nestrs |
|----------|--------|
| App / library / sub-app generators in a **monorepo** | **Not provided** — use [Cargo workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html) and `cargo new` / `cargo new --lib` per crate. |
| **Publishable library** packages (`@nestjs/...`) | **Not provided** — publish normal Rust crates to crates.io or use path/git dependencies. |
| **`package.json` scripts** (`nest build`, custom scripts) | **Not provided** — use `cargo build`, `cargo run --bin`, [cargo-make](https://github.com/sagiegurari/cargo-make), [just](https://github.com/casey/just), or Make. |
| `nest generate` (controllers, modules, CRUD, …) | **Partial overlap** — `nestrs generate` with a Rust- and nestrs-specific template set. |
| Plugins extending the CLI | **Not supported** — fork or wrap this crate if you need custom generators. |

This CLI focuses on **file/code generation** and a **single-crate** `nestrs new` skeleton. It does **not** replace Cargo for workspaces, libraries, or task runners.

## License

MIT OR Apache-2.0.
