# cargo-fuzz: `nestrs`

Requires **nightly** Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

Install **`cargo-fuzz` 0.13.1** (same as **`.github/workflows/fuzz.yml`**) **without** **`--locked`**. The crates.io release bundles a lockfile that pins **`rustix`** versions incompatible with recent nightly compilers.

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --version 0.13.1
cd nestrs/fuzz
cargo +nightly fuzz run authorization_bearer
cargo +nightly fuzz run uri_path_json
```

CI runs these targets on a schedule (see `.github/workflows/fuzz.yml`).
