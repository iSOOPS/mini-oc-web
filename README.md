# mini-oc-web

A Rust BFF (Axum) + Vue 3 SPA (placeholder — full UI in follow-up PR) for browsing mini-oc-gui (oc serve) sessions across devices.

## Quick Start

```bash
# Clone the repository
git clone <repository-url>
cd mini-oc-web

# Copy environment configuration
cp .env.example .env

# Edit .env and fill in your credentials

# Run the server
cargo run

# Open in browser
open http://127.0.0.1:8100
```

## Docker

```bash
# Build the Docker image
docker build -t mini-oc-web .

# Run with docker compose
docker compose up -d
```

## Documentation

- [Implementation Design](docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md)
- [Implementation Plan](docs/superpowers/plans/2026-08-28-mini-oc-web-impl.md)
