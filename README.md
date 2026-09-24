# mini-oc-web

A Rust BFF (Axum) + Vue 3 SPA (placeholder — full UI in follow-up PR) for browsing mini-oc-gui (oc serve) sessions across devices.

## Quick Start

```bash
# Clone the repository
git clone <repository-url>
cd mini-oc-web

# Copy environment configuration
cp .env.example .env
# Edit .env: set SB_PASSWORD + SB_BASE_URL (device-hop credentials are the
# portal user's own id + login key — no OPENCODE_SERVER_* vars needed)

# Build SPA (one-time, OR rely on Docker multi-stage build)
cd web && npm install && npm run build && cd ..

# Run the server
cargo run

# Open in browser
open http://127.0.0.1:8100
```

**Without SilverBullet (local-only dev):** set `SB_BASE_URL=http://127.0.0.1:65535` (unreachable). The BFF will start (warning logged) and return `503 service_unavailable` for `/api/devices` and `/api/config` while `/healthz` and `/` (SPA) keep working.

**Test the BFF in isolation:**

```bash
# Unit + integration tests (47 passing)
cargo test
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

## Local Debug Findings (Phase 1 MVP)

Bugs found and fixed during local smoke-test (see commit `b65cb4b`):

| Issue | Root cause | Fix |
|---|---|---|
| BFF wouldn't start without SB | `sb.login().await?` propagated startup error | Wrap SB login with `tracing::warn!` + continue (spec §14 stale-cache) |
| `/api/devices` returned `500 internal` when SB unreachable | Network errors mapped to generic `Internal` | Add `ServiceUnavailable` (503) variant; map reqwest connect/timeout errors |
| `/api/devices/{pctype}/{pcname}/projects` returned 500 when device offline | Same as above | Map to `DeviceOffline` (502) per spec §14 |
| `static_handler` always returned 112-byte placeholder; JS files served as `text/html` | Handler was a stub | Replace with `tower-http::services::ServeDir` + SPA fallback to `index.html` |
| Dockerfile couldn't build SPA (no node stage) | Single-stage rust + COPY of pre-built dist/ | Add `node:20-alpine` spa-builder stage; COPY from `spa-builder` stage |

## API Reference

See [`docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md`](docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md) §11 for the full BFF API matrix (11 endpoints).

| Error code | HTTP | Meaning |
|---|---|---|
| `unauthorized` | 401 | Missing/invalid cookie |
| `invalid_pcname` | 400 | `pcname` failed whitelist (`[A-Za-z0-9_-]{1,64}`) |
| `invalid_target` | 400 | `pctype` not in `{macos,windows}` or jump target unverified |
| `not_found` | 404 | Device / session / path not found |
| `rate_limited` | 429 | 5+ login failures within 10 min from same IP |
| `device_auth_failed` | 502 | Device returned 401/403 (unified credentials mismatch) |
| `device_offline` | 502 | Device unreachable (connect/timeout) |
| `service_unavailable` | 503 | SilverBullet unreachable |
| `internal` | 500 | Other server errors (parse failures, etc.) |

## Documentation

- [Implementation Design](docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md)
- [Implementation Plan](docs/superpowers/plans/2026-08-28-mini-oc-web-impl.md)
