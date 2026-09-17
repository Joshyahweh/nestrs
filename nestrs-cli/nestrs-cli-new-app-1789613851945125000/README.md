# nestrs-cli-new-app-1789613851945125000

Generated with `nestrs-cli new`.

## Development

```bash
cargo run
```

Server defaults:
- App: `http://127.0.0.1:3000/api`
- Health: `http://127.0.0.1:3000/health`
- Metrics: `http://127.0.0.1:3000/metrics`

## Production profile

```bash
NESTRS_ENV=production RUST_LOG=info cargo run --release
```

## Docker

```bash
docker build -t nestrs-cli-new-app-1789613851945125000:latest .
docker run -p 3000:3000 --env NESTRS_ENV=production nestrs-cli-new-app-1789613851945125000:latest
```

## Operations notes

- `enable_production_errors_from_env()` sanitizes 5xx responses in production.
- Configure CORS/security headers/rate limits per deployment needs.
- Run DB migrations/seeds before release rollout when using a real database.
