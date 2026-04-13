# cargo-fuzz: `nestrs-microservices`

Requires **nightly** Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cd nestrs-microservices/fuzz
cargo +nightly fuzz run wire_json
```

CI runs this target on a schedule (see `.github/workflows/fuzz.yml`).
