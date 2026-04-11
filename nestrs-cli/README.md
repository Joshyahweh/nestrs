# nestrs-scaffold

**CLI scaffolding** for [nestrs](https://crates.io/crates/nestrs): generate modules, resources, controllers, and DTOs (Nest-style file layout or idiomatic Rust filenames).

The crates.io package name is **`nestrs-scaffold`** because the **`nestrs-cli`** name is taken. The installed binary is still **`nestrs`**.

**Docs:** [docs.rs/nestrs-scaffold](https://docs.rs/nestrs-scaffold) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

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

## License

MIT OR Apache-2.0.
