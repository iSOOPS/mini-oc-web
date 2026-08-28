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

## Deploy to Server

1. **Push to git**: `git push origin main`
2. **Clone on server**:
   ```bash
   cd /srv/8.159.159.138  # or actual compose parent directory
   git clone <git_url>/mini-oc-web.git ../mini-oc-web
   ```
3. **Update server config** (see spec §12):
   - `docker-compose.yml` — append `mini-oc-web` service block (already done in Task 14)
   - `nginx/nginx.conf` — append upstream + 80/443 server blocks (already done in Task 14)
   - Replace `CHANGE_ME_BEFORE_DEPLOY` placeholders with actual passwords
4. **Build and start**:
   ```bash
   docker compose build mini-oc-web
   docker compose up -d mini-oc-web
   ```
5. **Reload nginx**:
   ```bash
   docker compose exec nginx nginx -t
   docker compose exec nginx nginx -s reload
   ```
6. **Verify**:
   ```bash
   curl -I https://web.isoops.com/healthz  # expect 200
   ```

## Architecture

mini-oc-web is a **BFF (Backend-For-Frontend)** that:
- Authenticates users via HTTP HMAC-signed cookies (unified credentials with mini-oc-gui end-side)
- Aggregates device state by polling `/health` on registered devices and reading `devices.json` from SilverBullet
- Proxies project/session API calls to oc serve instances (Basic auth, server-side)
- Generates `base64url`-encoded deep-link jump URLs with override priority (config.md > devices.json)

See [`docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md`](docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md) for the full design.

## What's Not In Scope (Phase 1 MVP)

- **mini-oc-gui end-side changes** — device registration, settings UI, `/health` JSON format (design doc §8) is a separate PR
- **Full SPA UI** — current SPA is placeholder; full Vue 3 UI in follow-up PR
- **Phase 2 / Phase 3** — iframe deep-link preheat, nginx `auth_request` SSO

## Documentation

- [Implementation Design](docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md)
- [Implementation Plan](docs/superpowers/plans/2026-08-28-mini-oc-web-impl.md)
