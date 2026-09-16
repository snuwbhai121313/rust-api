# Stage 1 — build
FROM rust:1.80-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

# Stage 2 — runtime
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/anikoto-api /usr/local/bin/
ENV PORT=8080
EXPOSE 8080
CMD ["anikoto-api"]
