# mini-oc-web

**English** | [简体中文](README.zh-CN.md)

A self-hosted web portal for browsing [OpenCode](https://opencode.ai) sessions across your devices.

mini-oc-web is a **BFF (Backend-For-Frontend)** written in Rust (Axum) with a Vue 3 SPA:
it aggregates device state, proxies project/session APIs to each device's `oc serve`
instance, and generates secure deep-link jump URLs — so you can see and resume coding
sessions on any machine (macOS / Windows) from a single page, including from your phone.

## Architecture Overview

```
                ┌────────────────────────────────────────────┐
 Browser ──────►│  nginx (single domain, path-prefix router) │
                └───────┬────────────────────────────┬───────┘
                        │ / , /api/*                 │ /{b64pc}/<rest>
                        ▼                            ▼
                ┌───────────────┐            ┌──────────────┐
                │  mini-oc-web  │            │ rathole-<dev>│ (tunnel upstreams)
                │  BFF (Axum)   │            └──────┬───────┘
                └───┬───────┬───┘                   │
                    │       │                       ▼
        ┌───────────┘       └──────────┐   ┌─────────────────────┐
        ▼                               │   │ Device (mini-oc-gui)│
┌───────────────┐              ┌────────▼──┴──────────┐          │
│ SilverBullet  │              │  probe /status +     │◄─────────┘
│ (user registry│              │  proxy oc-serve API  │  oc serve + rathole
│  + storage)   │              └──────────────────────┘  on the device
└───────────────┘
```

- **Device side** (`mini-oc-gui`, separate project): runs `opencode serve` locally and a
  [rathole](https://github.com/rapiz1/rathole) tunnel client, exposing the local oc
  server through the cloud as `{cloud_ip}:{port}`.
- **SilverBullet** acts as the source of truth: a `users.json` registry document holds
  tenants, their login keys, device grants and per-user storage config.
- **The BFF** authenticates users, reads the registry, probes each device's `/status`,
  proxies project/session calls server-side, and builds deep-link URLs.

## Core Design Guide

### 1. BFF pattern: all device calls are server-side

The browser only ever talks to the portal origin. The BFF performs every outbound call
to devices (probe, project/session proxy) itself. This:

- bypasses `oc serve`'s CORS whitelist entirely,
- keeps HTTP Basic credentials out of the frontend, URLs and logs,
- lets the SPA stay a pure same-origin consumer.

### 2. Authentication: HMAC-signed cookies + rate limiting

- Cookie value is HMAC-SHA256 signed with a key that is randomly generated on first
  start and persisted under `COOKIE_KEY_PATH` (mode 0600).
- Cookie flags: `HttpOnly + Secure + SameSite=Lax`, 7-day validity with sliding renewal.
- Login throttling per source IP: 5 failures within 10 minutes → `429 rate_limited`
  (in-memory; single instance by design, Redis only if multi-instance ever happens).
- Separate admin console (`/admin`) with its own cookie session and
  super-admin password.

### 3. Multi-tenant user registry (SilverBullet as storage)

`web/opencode/config.md` in SilverBullet holds a `users.json`-shaped registry:

```json
{
  "version": 1,
  "users": [
    {
      "id": "100001",
      "name": "alice",
      "key": "<32-char login key>",
      "cloud_ip": "<rathole server ip>",
      "devices": [
        {
          "desc": "Office Mac",
          "name": "rathole_office_mac",
          "port": 12333,
          "oc-port": 9464,
          "device-name": "",
          "pctype": "macos",
          "bound": false,
          "public-url": "https://oc-mac.example.com"
        }
      ],
      "sb": { "base_url": "…", "username": "…", "password": "…" }
    }
  ]
}
```

- Users log in with `name + key` (the key is also the device-hop credential).
- Devices are *grants*: the admin assigns tunnel service name + port; the device binds
  itself later via `POST /api/device-bind` (writes `device-name`, flips `bound`).
- `public-url` is the device's tunnel address `http(s)://host[:port]` — an explicit
  port is allowed (`ip:port` tunnel endpoints) and is then used as-is. `oc-port` is
  optional: when set, browser jump URLs carry it as a path prefix
  (`{public-url}/{oc-port}/…`); when empty, the jump URL is the bare tunnel address.
- No caches: every request reads the registry straight from SilverBullet — a small
  user base makes TTL caches (and their staleness bugs) not worth it (ADR #20/#21).

### 4. Device state: one round-trip `/api/me`

`GET /api/me` returns the user's devices **enriched with runtime status**: the BFF
concurrently probes the TUI service address of every device (4s timeout each,
`futures::join_all`, batched) — `{public-url}:{port}/status`, or the bare
`{public-url}` when it carries its own port. The probe never fails the request — a
dead device just comes back as `online:false, reason:"…"`, and the SPA renders
red/orange/green dots from `available ∈ {-1, 0, 1}`.

### 5. Deep-link jump URLs (base64url, override priority)

```
{base_url}/{base64url_nopad(directory)}/session/{session_id}[?auth_token=…]
```

- `directory` is encoded as URL-safe base64 without padding, so Chinese/space paths
  survive every hop; the format is locked by unit tests (`jump.rs`, zero dependencies).
- `base_url` priority: per-device override > user config > registry `public-url`
  composed as `{public-url}[/{oc-port}]` (path prefix; omitted when `oc-port`
  is empty). An override only changes where the **browser** jumps; the BFF data
  path always goes through the device's TUI tunnel address (the server cannot
  reach a user's LAN).
- `auth_token` (base64 of `user:pass`) is appended only on the LAN-fallback link to
  spare users the browser's native Basic-auth popup.
- Open-redirect defense: jump targets must come from the registry/config union —
  arbitrary URLs are rejected.

### 6. Hardened input handling

- Path params (`pctype`, `pcname`, `device_id`) pass a whitelist
  (`pctype ∈ {macos, windows}`, `pcname` matches `[A-Za-z0-9_-]{1,64}`).
- `directory` is percent-encoded and forwarded verbatim.
- `base_url` must be `http(s)://host[:port]` — no path, no `javascript:`/`data:`.
- Uniform error envelope: `{"error":{"code":"…","message":"…"}}` (see table below).

### 7. oc-serve wire-format adaptation (proxy layer)

`proxy.rs` normalizes opencode's native API shapes server-side, so the SPA consumes
a clean contract:

| BFF contract | oc-serve native shape |
|---|---|
| create-session req `{directory, title}` | `{title, location: {directory}}` |
| create-session resp `{id, directory}` | `{data: {id, title, directory}}` |
| project list `[{path, lastOpenedAt}]` | `[{id, path, name, sessions: […]}]` |
| session list `[{id, title, updatedAt}]` | `[{id, title, directory, created_at, updated_at}]` |

### 8. Architecture decision records (ADR 1–23)

| # | Decision | Why |
|---|----------|-----|
| 1 | BFF proxies all device traffic | avoids CORS; credentials stay server-side |
| 2 | Rename `mini-co-web` → `mini-oc-web` | naming aligned with the repo |
| 3 | SPA ships as skeleton first | full SPA logic deferred to later PRs |
| 4 | TDD on pure-logic modules first | jump / validation / whitelists are critical paths |
| 5 | Server deploy files patch a user-managed dir | not committed to this repo |
| 6 | Rust + Axum + reqwest (rustls-tls) | small static image, no OpenSSL |
| 7 | Vue 3 + Vite + Pinia + vue-router | light output, mature ecosystem |
| 8 | Multi-stage Dockerfile (node + rust) | SPA embedded in image, no mounts |
| 9 | Manual session create identifies device via `?pcname=` query | no pcname in frontend store |
| 10 | Registry writes use POST, never GET | GET must not mutate server state |
| 11 | SPA falls back to local base64 if `/jump` fails | mirrors oc-side pattern; non-blocking |
| 12 | Integration tests on `axum-test` + `wiremock` | reuse existing deps, no `mockito` |
| 13 | Health probe sends zero credentials | explicit "no auth" semantics; no fake creds in logs |
| 14 | Frontend `Promise.allSettled`, batch ≤ 6 | N ≪ 6 in practice; Chrome same-origin conn limit |
| 15 | LoadingOverlay teleports to `<body>` | escapes parent z-index/overflow/transform contexts |
| 16 | Cancel button rendered only when cancelable | one component serves both load styles |
| 17 | First load is silent; spinner only on explicit refresh | first-visit spinner flash is poor UX |
| 18 | Cancellation confirmed via toast | cancel is a user action needing feedback |
| 19 | `/api/me` returns devices + runtime in one call | kills the N+1 refresh pattern |
| 20 | No UsersCache TTL over SilverBullet | staleness bugs > perf gain at this scale |
| 21 | No DevicesCache TTL over SilverBullet | same |
| 22 | Keep `/api/device-status/:port` | future single-device deep probe |
| 23 | BFF ignores client aborts during probes | reqwest cancellation plumbing not worth it |

## Repository Layout

```
mini-oc-web/
├── Cargo.toml                # bin + lib (mini_oc_web)
├── src/
│   ├── main.rs               # bootstrap: tracing, AppState, router, listener
│   ├── state.rs              # AppState::from_env — config + shared clients
│   ├── routes.rs             # all /api/* handlers + static SPA serving
│   ├── auth.rs               # login/logout, HMAC cookie, per-IP limiter
│   ├── users.rs              # tenant registry (load/save/validate, key gen)
│   ├── sb.rs                 # SilverBullet HTTP client (cookie-name derivation)
│   ├── probe.rs              # device /status probing (never errors out)
│   ├── proxy.rs              # oc-serve API client (projects/sessions/create)
│   ├── jump.rs               # deep-link URL builder (pure, unit-locked)
│   └── error.rs              # AppError → HTTP status + error envelope
├── web/                      # Vue 3 SPA (Vite + TS + Pinia)
│   └── src/
│       ├── pages/            # Login, Devices, Projects, Sessions, Admin…
│       └── components/       # dialogs, loading overlay
└── tests/                    # integration suites (wiremock/mocks)
```

Module rule: dependencies are one-way, no cycles; `jump.rs` is dependency-free pure
logic; `sb.rs` depends only on reqwest/tokio; `routes.rs` is the single composition point.

## API Reference

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| POST | `/api/login` | — | user login (name + key) → session cookie |
| POST | `/api/logout` | cookie | clear session |
| GET | `/api/me` | cookie | current user + devices **with runtime status** |
| GET | `/api/me/key` | cookie | re-display own login key |
| POST | `/api/me/name` | cookie | rename self |
| POST | `/api/me/sb` | cookie | update own SilverBullet connection config |
| POST | `/api/user/info` | key | device-side fetch of its owner's record |
| POST | `/api/device-bind` | key | device self-registers (`device-name`, `bound`) |
| GET | `/api/device-status/:port` | cookie | single-device deep probe |
| GET | `/api/devices/:pctype/:pcname/projects` | cookie | proxy: project list |
| GET | `/api/devices/:pctype/:pcname/sessions` | cookie | proxy: session list |
| POST | `/api/devices/:pctype/:pcname/sessions` | cookie | proxy: create session |
| GET | `/api/devices/:pctype/:pcname/detail` | cookie | device conn detail (Basic creds) |
| GET | `/api/devices/:pctype/:pcname/jump` | cookie | build deep-link (302 or `?format=json`) |
| POST | `/api/admin/login` / `/api/admin/logout` | — / admin | admin session |
| GET | `/api/admin/info` | admin | runtime info (bind, SB status, uptime…) |
| GET | `/api/admin/users` | admin | list tenants |
| POST | `/api/admin/users` | admin | create tenant (auto key) |
| PUT | `/api/admin/users/:id` | admin | update tenant |
| POST | `/api/admin/users/:id/regenerate-key` | admin | roll login key |
| GET | `/healthz` | — | liveness probe |

Error codes (uniform envelope `{"error":{"code","message"}}`):

| Error code | HTTP | Meaning |
|---|---|---|
| `unauthorized` | 401 | Missing/invalid cookie |
| `invalid_pcname` | 400 | `pcname` failed whitelist (`[A-Za-z0-9_-]{1,64}`) |
| `invalid_target` | 400 | `pctype` not in `{macos,windows}` or jump target unverified |
| `not_found` | 404 | Device / session / path not found |
| `rate_limited` | 429 | 5+ login failures within 10 min from same IP |
| `device_auth_failed` | 502 | Device returned 401/403 (credentials mismatch) |
| `device_offline` | 502 | Device unreachable (connect/timeout) |
| `service_unavailable` | 503 | SilverBullet unreachable |
| `internal` | 500 | Other server errors (parse failures, etc.) |

## Quick Start

```bash
# Clone the repository
git clone <repository-url>
cd mini-oc-web

# Copy environment configuration
cp .env.example .env
# Edit .env: set SB_PASSWORD + SB_BASE_URL

# Build SPA (one-time, OR rely on Docker multi-stage build)
cd web && npm install && npm run build && cd ..

# Run the server
cargo run

# Open in browser
open http://127.0.0.1:8100
```

**Without SilverBullet (local-only dev):** set `SB_BASE_URL=http://127.0.0.1:65535`
(unreachable). The BFF will start (warning logged) and return `503 service_unavailable`
for registry-backed endpoints while `/healthz` and `/` (SPA) keep working.

## Configuration

All settings come from environment variables (see [`.env.example`](.env.example)):

| Variable | Default | Purpose |
|---|---|---|
| `WEB_PORT` / `WEB_BIND` | `8100` / `0.0.0.0` | HTTP listener |
| `COOKIE_KEY_PATH` | `/data/cookie_key` | HMAC key file (auto-generated, 0600) |
| `WEB_STATIC_DIR` | `./web/dist` | built SPA directory to serve |
| `SB_BASE_URL` | `http://127.0.0.1:3000` | SilverBullet endpoint |
| `SB_USER` / `SB_PASSWORD` | — | SilverBullet account (password doubles as admin console password) |
| `PORTAL_BASE` | `http://127.0.0.1:8100` | public origin used to build deep-link URLs |

## Docker

```bash
# Build the Docker image (multi-stage: node SPA build + rust release build)
docker build -t mini-oc-web .

# Run with docker compose (self-contained: bridge network + data volume)
docker compose up -d
```

Deployment notes:

- The HMAC cookie key lives in the `mini-oc-web-data` named volume (`/data`).
  Losing it invalidates every session — keep the volume persistent.
- Replace the `changeme` default (`SB_PASSWORD`) before any real deployment.

For production, put the portal behind your own reverse proxy (nginx/Caddy) on a single
domain and dispatch device deep-link prefixes (`/{b64pc}/…`) to the matching rathole
upstream — deployment config is managed outside this repo.

## Testing

```bash
cargo test          # unit + integration suites (wiremock/mocks, no real devices needed)
cd web && npm run build   # vue-tsc type-check + vite build
```

## Roadmap

- **Phase 2** — iframe deep-link preheat (eliminate cold-start fallback), path-list
  section metadata upgrade
- **Phase 3** — nginx `auth_request` full-domain SSO

## License

[MIT](LICENSE)
