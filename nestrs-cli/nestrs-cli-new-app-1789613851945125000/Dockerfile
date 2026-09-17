FROM rust:1.75 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/nestrs-cli-new-app-1789613851945125000 /usr/local/bin/nestrs-cli-new-app-1789613851945125000
ENV NESTRS_ENV=production
ENV PORT=3000
EXPOSE 3000
HEALTHCHECK CMD curl -f http://localhost:3000/health || exit 1
CMD ["/usr/local/bin/nestrs-cli-new-app-1789613851945125000"]
