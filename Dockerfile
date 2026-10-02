# ---------- SPA builder ----------
FROM harbor.nnkcy.com/open-library/node:23-alpine AS spa-builder
ENV NPM_CONFIG_REGISTRY=https://registry.npmmirror.com
WORKDIR /spa
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ ./
RUN npm run build

# ---------- Rust builder ----------
FROM harbor.nnkcy.com/open-library/rust:1.85-slim AS builder
WORKDIR /app

# Keep cargo re-resolve MSRV-aware so a stale lockfile can never pull crate
# versions newer than the rustc in this image supports.
ENV CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback

# China mirror for crate downloads; lockfile source URLs and checksums stay untouched.
RUN printf '[source.crates-io]\nreplace-with = "rsproxy-sparse"\n\n[source.rsproxy-sparse]\nregistry = "sparse+https://rsproxy.cn/index/"\n' > "$CARGO_HOME/config.toml"

RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
        pkg-config && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src

COPY src ./src
# COPY keeps old checkout mtimes, so without this touch cargo sees the dummy
# binary from the previous layer as fresh and ships it instead of real code.
RUN touch src/main.rs src/lib.rs && cargo build --release

# ---------- runtime ----------
FROM harbor.nnkcy.com/open-library/debian:bookworm-slim
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates telnet wget && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/mini-oc-web /usr/local/bin/mini-oc-web
COPY --from=spa-builder /spa/dist /app/web

ENV WEB_STATIC_DIR=/app/web
ENV COOKIE_KEY_PATH=/data/cookie_key
EXPOSE 8100
WORKDIR /app
ENTRYPOINT ["/usr/local/bin/mini-oc-web"]