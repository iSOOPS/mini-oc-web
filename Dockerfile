# ---------- builder ----------
FROM rust:1.83-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config && rm -rf /var/lib/apt/lists/*

# Dep cache layer
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src

# Source
COPY src ./src
RUN cargo build --release

# ---------- runtime ----------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates wget && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mini-oc-web /usr/local/bin/mini-oc-web
COPY web/dist /app/web

ENV WEB_STATIC_DIR=/app/web
ENV COOKIE_KEY_PATH=/data/cookie_key
EXPOSE 8100
WORKDIR /app
ENTRYPOINT ["/usr/local/bin/mini-oc-web"]
