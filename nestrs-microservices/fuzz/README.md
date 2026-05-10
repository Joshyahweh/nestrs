# cargo-fuzz: `nestrs-microservices`

Requires **nightly** Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

Install **`cargo-fuzz` 0.13.1** (same as **`.github/workflows/fuzz.yml`**) **without** **`--locked`**. The crates.io release bundles a lockfile that pins **`rustix`** versions incompatible with recent nightly compilers.

```bash
rustup toolchain install nightly
cargo install cargo-fuzz --version 0.13.1
cd nestrs-microservices/fuzz
cargo +nightly fuzz run wire_json
```

CI runs this target on a schedule (see `.github/workflows/fuzz.yml`).
