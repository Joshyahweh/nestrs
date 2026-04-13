# cargo-fuzz: `nestrs`

Requires **nightly** Rust and [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz).

```bash
rustup toolchain install nightly
cargo install cargo-fuzz
cd nestrs/fuzz
cargo +nightly fuzz run authorization_bearer
cargo +nightly fuzz run uri_path_json
```

CI runs these targets on a schedule (see `.github/workflows/fuzz.yml`).
