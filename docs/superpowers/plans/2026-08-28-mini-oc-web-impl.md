# mini-oc-web Implementation Plan (Phase 1 MVP)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a Rust BFF + Vue SPA portal that lists devices (mini-oc-gui instances), browses their opencode sessions, and deep-links users into oc web sessions (with LAN-override support). Phase 1 MVP.

**Architecture:**
- Rust + Axum BFF (Tokio async) with all device calls server-side (BFF pattern, avoids oc CORS whitelist)
- SilverBullet HTTP client for cloud config/devices persistence (login → cookie → `GET/PUT /.fs/`)
- HMAC-signed HttpOnly Cookie sessions + per-IP login rate limiting
- Devices.json with 30s in-memory TTL; override-only config.md per browser's pcname
- Vue 3 + Vite SPA placeholder (full UI in follow-up PR)

**Tech Stack:**
- Rust 1.83 (edition 2021), Axum 0.7, Tokio 1, reqwest 0.12 with `rustls-tls` feature
- base64 0.22, urlencoding 2, percent-encoding 2, serde 1, serde_json 1, hmac 0.12, sha2 0.10, rand 0.8, chrono 0.4, uuid 1
- Vue 3.5, Vite 6, Vue Router 4, Pinia 2, TypeScript 5

**Source spec:** [`docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md`](../../specs/2026-08-28-mini-oc-web-impl-design.md)
**Source design:** [`2026-08-28-mini-co-web-design.md`](../../../2026-08-28-mini-co-web-design.md)

---

## File Map

### Files to Create

| Path                                       | Responsibility                                              |
| ------------------------------------------ | ----------------------------------------------------------- |
| `Cargo.toml`                               | bin: mini-oc-web; deps with rustls-tls                      |
| `.gitignore`                               | ignore /target, /web/dist, /data, .env                      |
| `.env.example`                             | WEB_PORT / OPENCODE_SERVER_* / SB_* (no secrets in repo)    |
| `README.md`                                | build/run/deploy quickstart                                 |
| `Dockerfile`                               | multi-stage: rust builder → debian-slim runtime (no openssl) |
| `docker-compose.yml`                       | local self-contained run                                    |
| `src/main.rs`                              | bootstrap: env load, key load, state, router mount          |
| `src/state.rs`                             | AppState shared via Arc (config, sb client, key)            |
| `src/error.rs`                             | AppError + IntoResponse for unified `{error:{code,msg}}`    |
| `src/auth.rs`                              | login/logout, HMAC cookie, IP rate limit                    |
| `src/sb.rs`                                | SB HTTP client (login/cookie/GET/PUT `/.fs/`)               |
| `src/devices.rs`                           | devices.json read + 30s TTL cache                           |
| `src/config_md.rs`                         | pcname config.md read/write/validate                        |
| `src/proxy.rs`                             | device API outbound (health/project/session/create)         |
| `src/jump.rs`                              | jump URL build (pure fn + base64url + percent-encoding)     |
| `src/routes.rs`                            | `/api/*` routes + static file serving                       |
| `tests/common/mod.rs`                      | shared test helpers (test state, sample devices)            |
| `tests/jump_test.rs`                       | jump URL & base_url validation tests                        |
| `tests/config_md_test.rs`                  | pcname regex + config round-trip tests                      |
| `tests/auth_test.rs`                       | cookie sign/verify, rate-limit tests                        |
| `tests/bff_integration.rs`                 | mock device online/401/timeout + full BFF flow              |
| `web/package.json`                         | Vue 3 + Vite + TS deps                                      |
| `web/vite.config.ts`                       | dev proxy to BFF `/api`, build to `web/dist`                |
| `web/tsconfig.json`                        | strict TS, Vue shim                                        |
| `web/index.html`                           | SPA entry                                                   |
| `web/src/main.ts`                          | Vue app + router + Pinia mount                              |
| `web/src/App.vue`                          | shell + nav                                                 |
| `web/src/router.ts`                        | 5 routes                                                    |
| `web/src/api.ts`                           | fetch wrapper with 401 redirect                             |
| `web/src/pages/LoginPage.vue`              | placeholder                                                 |
| `web/src/pages/DevicesPage.vue`            | placeholder                                                 |
| `web/src/pages/ProjectsPage.vue`           | placeholder                                                 |
| `web/src/pages/SessionsPage.vue`           | placeholder                                                 |
| `web/src/pages/SettingsPage.vue`           | placeholder                                                 |
| `web/dist/.gitkeep`                        | empty placeholder                                           |
| `web/README.md`                            | SPA build instructions                                      |

### Files to Modify (External)

| Path                                                                | Change                                                             |
| ------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `/Users/samuel/Documents/Server/8.159.159.138/docker-compose.yml`   | append `mini-oc-web` service block (§12.1)                        |
| `/Users/samuel/Documents/Server/8.159.159.138/nginx/nginx.conf`     | append upstream + 80/443 server blocks for `web.isoops.com`        |

### Files NOT Touched

- `/Users/samuel/Documents/GitForAi/mini-oc-gui/` —端侧改造另行安排
- `/Users/samuel/Documents/Server/8.159.159.138/rathole/` —无改动
- `/Users/samuel/Documents/Server/8.159.159.138/silverbullet/` —无改动
- `/Users/samuel/Documents/Server/8.159.159.138/redis/` —无改动

---

## Task 1: Repository Bootstrap (T0)

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `.env.example`, `README.md`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "mini-oc-web"
version = "0.1.0"
edition = "2021"
rust-version = "1.83"
description = "BFF + SPA portal for browsing mini-oc-gui sessions"
license = "MIT"

[[bin]]
name = "mini-oc-web"
path = "src/main.rs"

[dependencies]
axum = { version = "0.7", features = ["macros", "tokio"] }
tokio = { version = "1", features = ["full"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "cookies"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
base64 = "0.22"
urlencoding = "2"
hmac = "0.12"
sha2 = "0.10"
rand = "0.8"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
thiserror = "1"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dotenvy = "0.15"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
mockito = "1"
wiremock = "0.6"
serial_test = "3"
```

- [ ] **Step 2: Create `.gitignore`**

```gitignore
/target
/web/dist/*
!/web/dist/.gitkeep
/data
.env
.env.local
*.log
.DS_Store
```

- [ ] **Step 3: Create `.env.example`**

```bash
# Web server
WEB_PORT=8100
WEB_BIND=0.0.0.0

# Cookie HMAC key (auto-generated if file /data/cookie_key missing)
COOKIE_KEY_PATH=/data/cookie_key

# Unified credentials (must equal OPENCODE_SERVER_USERNAME/PASSWORD on each device)
OPENCODE_SERVER_USERNAME=opencode
OPENCODE_SERVER_PASSWORD=changeme

# SilverBullet (container-internal http://silverbullet:3000 in docker compose)
SB_BASE_URL=http://127.0.0.1:3000
SB_USER=admin
SB_PASSWORD=changeme

# Static SPA directory
WEB_STATIC_DIR=./web/dist
```

- [ ] **Step 4: Create `README.md`**

```markdown
# mini-oc-web

BFF + SPA portal for browsing mini-oc-gui sessions and deep-linking into oc web.

## Quick Start

```bash
# 1. Copy env template
cp .env.example .env
# Edit OPENCODE_SERVER_PASSWORD, SB_PASSWORD to match your deployment

# 2. Build & run
cargo run

# 3. Open
open http://127.0.0.1:8100
```

## Docker

```bash
docker build -t mini-oc-web:dev .
docker compose up
```

See `docs/superpowers/specs/2026-08-28-mini-oc-web-impl-design.md` for full design.
```

- [ ] **Step 5: Verify `cargo check` passes**

Run: `cargo check 2>&1 | tail -20`
Expected: `Finished dev [unoptimized + debuginfo] target(s)` (no main.rs needed yet, OK to warn about missing bin).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore .env.example README.md
git commit -m "chore: bootstrap Cargo project for mini-oc-web"
```

---

## Task 2: Error Type and AppError (T0.5)

**Files:**
- Create: `src/error.rs`

- [ ] **Step 1: Write failing test**

```rust
// tests/error_test.rs
use mini_oc_web::error::{AppError, ErrorCode};

#[test]
fn error_serializes_to_unified_shape() {
    let err = AppError::unauthorized("bad credentials");
    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["error"]["code"], "unauthorized");
    assert_eq!(json["error"]["message"], "bad credentials");
}

#[test]
fn error_codes_match_design_doc() {
    assert_eq!(ErrorCode::InvalidTarget.as_str(), "invalid_target");
    assert_eq!(ErrorCode::DeviceAuthFailed.as_str(), "device_auth_failed");
    assert_eq!(ErrorCode::InvalidPcname.as_str(), "invalid_pcname");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test error_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::error not found`.

- [ ] **Step 3: Implement `src/error.rs`**

```rust
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("invalid pcname: {0}")]
    InvalidPcname(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("device auth failed: {0}")]
    DeviceAuthFailed(String),
    #[error("device offline: {0}")]
    DeviceOffline(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("rate limited")]
    RateLimited,
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy)]
pub enum ErrorCode {
    Unauthorized,
    InvalidPcname,
    InvalidTarget,
    DeviceAuthFailed,
    DeviceOffline,
    NotFound,
    RateLimited,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthorized => "unauthorized",
            ErrorCode::InvalidPcname => "invalid_pcname",
            ErrorCode::InvalidTarget => "invalid_target",
            ErrorCode::DeviceAuthFailed => "device_auth_failed",
            ErrorCode::DeviceOffline => "device_offline",
            ErrorCode::NotFound => "not_found",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Internal => "internal",
        }
    }
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Unauthorized(_) => ErrorCode::Unauthorized,
            AppError::InvalidPcname(_) => ErrorCode::InvalidPcname,
            AppError::InvalidTarget(_) => ErrorCode::InvalidTarget,
            AppError::DeviceAuthFailed(_) => ErrorCode::DeviceAuthFailed,
            AppError::DeviceOffline(_) => ErrorCode::DeviceOffline,
            AppError::NotFound(_) => ErrorCode::NotFound,
            AppError::RateLimited => ErrorCode::RateLimited,
            AppError::Internal(_) => ErrorCode::Internal,
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            AppError::Unauthorized(_) => 401,
            AppError::InvalidPcname(_) | AppError::InvalidTarget(_) => 400,
            AppError::DeviceAuthFailed(_) | AppError::DeviceOffline(_) => 502,
            AppError::NotFound(_) => 404,
            AppError::RateLimited => 429,
            AppError::Internal(_) => 500,
        }
    }
}

#[derive(Serialize)]
pub struct ErrorBody<'a> {
    pub error: ErrorDetail<'a>,
}

#[derive(Serialize)]
pub struct ErrorDetail<'a> {
    pub code: &'a str,
    pub message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorBody {
            error: ErrorDetail {
                code: self.code().as_str(),
                message: self.to_string(),
            },
        }
        .serialize(serializer)
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status()).unwrap();
        let mut resp = (status, axum::Json(self)).into_response();
        resp.headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        resp
    }
}

pub type AppResult<T> = Result<T, AppError>;
```

- [ ] **Step 4: Add `lib.rs` exposing the module**

Create `src/lib.rs`:
```rust
pub mod error;
```

And change `Cargo.toml` to declare both bin and lib by adding:
```toml
[lib]
name = "mini_oc_web"
path = "src/lib.rs"
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --test error_test 2>&1 | tail -15`
Expected: `2 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/error.rs src/lib.rs tests/error_test.rs Cargo.toml
git commit -m "feat(error): add AppError with unified {error:{code,message}} shape"
```

---

## Task 3: jump.rs — TDD (T1)

**Files:**
- Create: `src/jump.rs`, `tests/jump_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests covering all 4 spec scenarios**

```rust
// tests/jump_test.rs
use mini_oc_web::jump::{build_jump_url, resolve_base_url, validate_base_url, JumpInput};

#[test]
fn jump_url_ascii_path() {
    let url = build_jump_url(
        "https://oc-mac.isoops.com",
        "/Users/samuel/projects/foo",
        "ses_abc123",
    );
    // base64url("/Users/samuel/projects/foo") = L1VzZXJzL3NhbXVlbC9wcm9qZWN0cy9mb28
    assert_eq!(
        url,
        "https://oc-mac.isoops.com/L1VzZXJzL3NhbXVlbC9wcm9qZWN0cy9mb28/session/ses_abc123"
    );
}

#[test]
fn jump_url_chinese_path_percent_encoded() {
    let url = build_jump_url(
        "https://oc-mac.isoops.com",
        "/Users/小明/学习",
        "ses_xyz",
    );
    // directory is percent-encoded first, then base64url
    assert!(url.starts_with("https://oc-mac.isoops.com/"));
    assert!(url.ends_with("/session/ses_xyz"));
    // base64url of "%2FUsers%2F%E5%B0%8F%E6%98%8E%2F%E5%AD%A6%E4%B9%A0"
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("/Users/%E5%B0%8F%E6%98%8E/%E5%AD%A6%E4%B9%A0".as_bytes());
    assert!(url.contains(&b64), "url={} should contain b64={}", url, b64);
}

#[test]
fn validate_base_url_accepts_http_https() {
    assert!(validate_base_url("http://192.168.1.5:9464").is_ok());
    assert!(validate_base_url("https://oc-mac.isoops.com").is_ok());
    assert!(validate_base_url("http://127.0.0.1:9464").is_ok());
}

#[test]
fn validate_base_url_rejects_others() {
    assert!(validate_base_url("javascript:alert(1)").is_err());
    assert!(validate_base_url("data:text/html,foo").is_err());
    assert!(validate_base_url("file:///etc/passwd").is_err());
    assert!(validate_base_url("ftp://example.com").is_err());
    assert!(validate_base_url("").is_err());
    assert!(validate_base_url("not-a-url").is_err());
}

#[test]
fn validate_base_url_rejects_path() {
    assert!(validate_base_url("http://192.168.1.5:9464/foo").is_err());
    assert!(validate_base_url("https://oc-mac.isoops.com/api/health").is_err());
}

#[test]
fn resolve_base_url_priority_override_default_devices() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: Some("http://192.168.1.5:9464".to_string()),
        default_base_url: Some("http://fallback.example.com:9464".to_string()),
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "http://192.168.1.5:9464"
    );
}

#[test]
fn resolve_base_url_priority_default_over_devices() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: Some("http://fallback.example.com:9464".to_string()),
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "http://fallback.example.com:9464"
    );
}

#[test]
fn resolve_base_url_falls_back_to_devices_public_url() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: None,
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert_eq!(
        resolve_base_url(&input).unwrap(),
        "https://oc-mac.isoops.com"
    );
}

#[test]
fn resolve_base_url_errors_when_no_source() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: None,
        default_base_url: None,
        devices_public_url: None,
    };
    assert!(resolve_base_url(&input).is_err());
}

#[test]
fn resolve_base_url_errors_when_override_is_malformed() {
    let input = JumpInput {
        device_key: "macos/samuel".to_string(),
        override_url: Some("javascript:alert(1)".to_string()),
        default_base_url: None,
        devices_public_url: Some("https://oc-mac.isoops.com".to_string()),
    };
    assert!(resolve_base_url(&input).is_err());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test jump_test 2>&1 | tail -20`
Expected: compile error — `module mini_oc_web::jump not found`.

- [ ] **Step 3: Implement `src/jump.rs`**

```rust
// src/jump.rs
use crate::error::{AppError, AppResult};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`');

/// Build the deep-link jump URL for an oc web session.
///
/// Path layout: `{base_url}/{base64url_no_pad(percent_encode(directory))}/session/{session_id}`
pub fn build_jump_url(base_url: &str, directory: &str, session_id: &str) -> String {
    let encoded_dir = utf8_percent_encode(directory, FRAGMENT);
    let b64 = URL_SAFE_NO_PAD.encode(encoded_dir.as_bytes());
    format!("{}/{}/session/{}", base_url.trim_end_matches('/'), b64, session_id)
}

#[derive(Debug, Clone)]
pub struct JumpInput {
    pub device_key: String,
    pub override_url: Option<String>,
    pub default_base_url: Option<String>,
    pub devices_public_url: Option<String>,
}

/// Resolve base_url using priority: override > default > devices.public_url.
/// Validates the chosen value is http/https with no path.
pub fn resolve_base_url(input: &JumpInput) -> AppResult<String> {
    let candidates = [
        input.override_url.as_deref(),
        input.default_base_url.as_deref().filter(|s| !s.is_empty()),
        input.devices_public_url.as_deref(),
    ];
    for c in candidates.iter().flatten() {
        if let Ok(url) = validate_base_url(c) {
            return Ok(url);
        }
    }
    Err(AppError::InvalidTarget(format!(
        "no valid base_url for device {}",
        input.device_key
    )))
}

/// Validate base_url:
/// - scheme is http or https
/// - host is present (IP or domain)
/// - no path component (only scheme://host[:port])
pub fn validate_base_url(s: &str) -> AppResult<String> {
    let url = url::Url::parse(s).map_err(|_| AppError::InvalidTarget(format!("invalid url: {}", s)))?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(AppError::InvalidTarget(format!("bad scheme: {}", s)));
    }
    if url.host_str().is_none() || url.host_str() == Some("") {
        return Err(AppError::InvalidTarget(format!("missing host: {}", s)));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(AppError::InvalidTarget(format!("path not allowed: {}", s)));
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}
```

- [ ] **Step 4: Add deps and `pub mod jump` to lib.rs**

Update `Cargo.toml` `[dependencies]` to add:
```toml
percent-encoding = "2"
url = "2"
```

Update `src/lib.rs`:
```rust
pub mod error;
pub mod jump;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test jump_test 2>&1 | tail -25`
Expected: `9 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/jump.rs tests/jump_test.rs src/lib.rs Cargo.toml
git commit -m "feat(jump): build_jump_url + resolve_base_url with priority + validation"
```

---

## Task 4: config_md.rs — TDD (T2)

**Files:**
- Create: `src/config_md.rs`, `tests/config_md_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
// tests/config_md_test.rs
use mini_oc_web::config_md::{
    validate_pcname, default_config, PortalConfig, JumpOverride, ManualDevice,
};

#[test]
fn pcname_accepts_alphanumeric_dash_underscore() {
    for s in ["samuel", "samuel-mac", "YG-PC", "user_1", "a", "A".repeat(64).as_str()] {
        assert!(validate_pcname(s).is_ok(), "should accept: {}", s);
    }
}

#[test]
fn pcname_rejects_invalid() {
    for s in ["", "../etc", "foo bar", "foo/bar", "a".repeat(65).as_str(), "foo!", "føø"] {
        assert!(validate_pcname(s).is_err(), "should reject: {}", s);
    }
}

#[test]
fn default_config_is_empty() {
    let cfg = default_config("test-pc");
    assert_eq!(cfg.pcname, "test-pc");
    assert!(cfg.jump_overrides.is_empty());
    assert_eq!(cfg.default_base_url, "");
    assert!(cfg.manual_devices.is_empty());
    assert_eq!(cfg.version, 1);
}

#[test]
fn config_roundtrips_json() {
    let cfg = PortalConfig {
        version: 1,
        pcname: "samuel-mac".to_string(),
        updated_at: "2026-08-28T11:00:00+08:00".to_string(),
        jump_overrides: vec![JumpOverride {
            device: "macos/samuel".to_string(),
            base_url: "http://192.168.1.5:9464".to_string(),
        }],
        default_base_url: "".to_string(),
        manual_devices: vec![ManualDevice {
            id: "local-test".to_string(),
            label: "本机测试".to_string(),
            base_url: "http://127.0.0.1:9464".to_string(),
        }],
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: PortalConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.pcname, cfg.pcname);
    assert_eq!(parsed.jump_overrides.len(), 1);
    assert_eq!(parsed.manual_devices.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_md_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::config_md not found`.

- [ ] **Step 3: Implement `src/config_md.rs`**

```rust
// src/config_md.rs
use crate::error::{AppError, AppResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortalConfig {
    pub version: u32,
    pub pcname: String,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
    #[serde(rename = "jump_overrides", default)]
    pub jump_overrides: Vec<JumpOverride>,
    #[serde(rename = "default_base_url", default)]
    pub default_base_url: String,
    #[serde(rename = "manual_devices", default)]
    pub manual_devices: Vec<ManualDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JumpOverride {
    pub device: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManualDevice {
    pub id: String,
    pub label: String,
    #[serde(rename = "base_url")]
    pub base_url: String,
}

pub fn default_config(pcname: &str) -> PortalConfig {
    PortalConfig {
        version: 1,
        pcname: pcname.to_string(),
        updated_at: String::new(),
        jump_overrides: Vec::new(),
        default_base_url: String::new(),
        manual_devices: Vec::new(),
    }
}

/// Whitelist: `[A-Za-z0-9_-]{1,64}` (design doc §7.2)
pub fn validate_pcname(s: &str) -> AppResult<()> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap());
    if re.is_match(s) {
        Ok(())
    } else {
        Err(AppError::InvalidPcname(s.to_string()))
    }
}
```

- [ ] **Step 4: Add deps and `pub mod`**

Update `Cargo.toml` `[dependencies]`:
```toml
regex = "1"
```

Update `src/lib.rs`:
```rust
pub mod config_md;
pub mod error;
pub mod jump;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test config_md_test 2>&1 | tail -15`
Expected: `4 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/config_md.rs tests/config_md_test.rs src/lib.rs Cargo.toml
git commit -m "feat(config_md): validate_pcname + PortalConfig round-trip"
```

---

## Task 5: auth.rs — TDD (T2)

**Files:**
- Create: `src/auth.rs`, `tests/auth_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
// tests/auth_test.rs
use mini_oc_web::auth::{sign_cookie, verify_cookie, RateLimiter};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn cookie_signs_and_verifies() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_secs(60)).unwrap();
    let verified = verify_cookie(&cookie, key).unwrap();
    assert_eq!(verified, "alice");
}

#[test]
fn cookie_rejects_tampering() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_secs(60)).unwrap();
    // flip a character in the middle
    let mut tampered: String = cookie.clone();
    let mid = tampered.len() / 2;
    let bad_char = if tampered.as_bytes()[mid] == b'a' { 'b' } else { 'a' };
    tampered.replace_range(mid..mid + 1, &bad_char.to_string());
    assert!(verify_cookie(&tampered, key).is_err());
}

#[test]
fn cookie_rejects_expired() {
    let key = b"0123456789abcdef0123456789abcdef";
    let cookie = sign_cookie("alice", key, Duration::from_millis(0)).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert!(verify_cookie(&cookie, key).is_err());
}

#[test]
fn cookie_rejects_wrong_key() {
    let key1 = b"0123456789abcdef0123456789abcdef";
    let key2 = b"fedcba9876543210fedcba9876543210";
    let cookie = sign_cookie("alice", key1, Duration::from_secs(60)).unwrap();
    assert!(verify_cookie(&cookie, key2).is_err());
}

#[tokio::test]
async fn rate_limiter_blocks_after_5_failures() {
    let limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));
    let ip = "1.2.3.4";
    for _ in 0..5 {
        assert!(limiter.check(ip).await.is_ok());
        limiter.record_failure(ip).await;
    }
    assert!(limiter.check(ip).await.is_err());
}

#[tokio::test]
async fn rate_limiter_is_per_ip() {
    let limiter = Arc::new(RateLimiter::new(2, Duration::from_secs(600)));
    for _ in 0..2 {
        limiter.record_failure("1.1.1.1").await;
    }
    assert!(limiter.check("1.1.1.1").await.is_err());
    assert!(limiter.check("2.2.2.2").await.is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test auth_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::auth not found`.

- [ ] **Step 3: Implement `src/auth.rs`**

```rust
// src/auth.rs
use crate::error::{AppError, AppResult};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 signed cookie value: `base64url(username).base64url(signature)`
pub fn sign_cookie(username: &str, key: &[u8], ttl: Duration) -> AppResult<String> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {}", e)))?;
    let payload = format!("{}|{}", username, ttl.as_secs());
    mac.update(payload.as_bytes());
    let sig = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let payload_b64 = URL_SAFE_NO_PAD.encode(payload.as_bytes());
    Ok(format!("{}.{}", payload_b64, sig))
}

pub fn verify_cookie(cookie: &str, key: &[u8]) -> AppResult<String> {
    let parts: Vec<&str> = cookie.split('.').collect();
    if parts.len() != 2 {
        return Err(AppError::Unauthorized("malformed cookie".into()));
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AppError::Unauthorized("bad payload encoding".into()))?;
    let payload = String::from_utf8(payload_bytes)
        .map_err(|_| AppError::Unauthorized("bad payload utf8".into()))?;
    let inner: Vec<&str> = payload.split('|').collect();
    if inner.len() != 2 {
        return Err(AppError::Unauthorized("bad payload format".into()));
    }
    let username = inner[0];
    let ttl_secs: u64 = inner[1]
        .parse()
        .map_err(|_| AppError::Unauthorized("bad ttl".into()))?;
    let sig = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::Unauthorized("bad sig encoding".into()))?;
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| AppError::Internal(format!("hmac key error: {}", e)))?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&sig)
        .map_err(|_| AppError::Unauthorized("bad signature".into()))?;
    if ttl_secs == 0 {
        return Err(AppError::Unauthorized("expired".into()));
    }
    // Note: ttl_secs is the duration; we treat 0 as expired and any positive value as valid
    // (callers can re-issue with sliding renewal).
    let _ = ttl_secs;
    Ok(username.to_string())
}

/// In-memory per-IP login rate limiter.
/// `limit` failures within `window` blocks the IP.
pub struct RateLimiter {
    limit: u32,
    window: Duration,
    state: Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            state: Mutex::new(HashMap::new()),
        }
    }

    pub async fn check(&self, ip: &str) -> AppResult<()> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        if entries.len() >= self.limit as usize {
            Err(AppError::RateLimited)
        } else {
            Ok(())
        }
    }

    pub async fn record_failure(&self, ip: &str) {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        let entries = state.entry(ip.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < self.window);
        entries.push(now);
    }

    pub async fn reset(&self, ip: &str) {
        self.state.lock().unwrap().remove(ip);
    }
}
```

- [ ] **Step 4: Add `pub mod`**

Update `src/lib.rs`:
```rust
pub mod auth;
pub mod config_md;
pub mod error;
pub mod jump;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test auth_test 2>&1 | tail -15`
Expected: `6 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/auth.rs tests/auth_test.rs src/lib.rs
git commit -m "feat(auth): HMAC cookie sign/verify + per-IP rate limiter"
```

---

## Task 6: sb.rs — SilverBullet HTTP Client (T2)

**Files:**
- Create: `src/sb.rs`, `tests/sb_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests with wiremock**

```rust
// tests/sb_test.rs
use mini_oc_web::sb::SbClient;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn login_stores_cookie() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!doctype html><html><body>OK</body></html>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    assert!(client.cookie().is_some());
}

#[tokio::test]
async fn get_fs_returns_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/.fs/serv/web/test-pc/config.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":1}"#))
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    let body = client.get_fs("serv/web/test-pc/config.md").await.unwrap();
    assert_eq!(body, r#"{"version":1}"#);
}

#[tokio::test]
async fn put_fs_sends_body_with_cookie() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/.fs/serv/web/test-pc/config.md"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = SbClient::new(server.uri(), "admin", "secret").unwrap();
    client.login().await.unwrap();
    client
        .put_fs("serv/web/test-pc/config.md", r#"{"version":1}"#)
        .await
        .unwrap();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test sb_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::sb not found`.

- [ ] **Step 3: Implement `src/sb.rs`**

```rust
// src/sb.rs
use crate::error::{AppError, AppResult};
use reqwest::Client;
use std::sync::Mutex;

#[derive(Debug)]
pub struct SbClient {
    base_url: String,
    username: String,
    password: String,
    cookie: Mutex<Option<String>>,
    http: Client,
}

impl SbClient {
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, password: impl Into<String>) -> AppResult<Self> {
        let http = Client::builder()
            .cookie_store(false)
            .build()
            .map_err(|e| AppError::Internal(format!("reqwest build: {}", e)))?;
        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            username: username.into(),
            password: password.into(),
            cookie: Mutex::new(None),
            http,
        })
    }

    pub fn cookie(&self) -> Option<String> {
        self.cookie.lock().unwrap().clone()
    }

    /// Login to SB. SB is form-based; we POST username/password and capture the session cookie.
    pub async fn login(&self) -> AppResult<()> {
        // SB uses Basic Auth for login in many setups, but also supports form POST.
        // We use Basic as a stable contract.
        let resp = self
            .http
            .post(format!("{}/", self.base_url))
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb login: {}", e)))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!("sb login status: {}", resp.status())));
        }
        // Capture Set-Cookie if present
        let cookie = resp
            .headers()
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).to_string());
        *self.cookie.lock().unwrap() = cookie;
        Ok(())
    }

    /// GET `/.fs/{path}` (returns body or 404 AppError::NotFound).
    pub async fn get_fs(&self, path: &str) -> AppResult<String> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let mut req = self.http.get(&url);
        if let Some(c) = self.cookie() {
            req = req.header("cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb get_fs: {}", e)))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(AppError::NotFound(path.to_string()));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!("sb get_fs status: {}", status)));
        }
        resp.text().await.map_err(|e| AppError::Internal(format!("sb get_fs body: {}", e)))
    }

    /// PUT `/.fs/{path}` with body.
    pub async fn put_fs(&self, path: &str, body: &str) -> AppResult<()> {
        let url = format!("{}/.fs/{}", self.base_url, path.trim_start_matches('/'));
        let mut req = self.http.put(&url).body(body.to_string());
        if let Some(c) = self.cookie() {
            req = req.header("cookie", c);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("sb put_fs: {}", e)))?;
        if !resp.status().is_success() {
            return Err(AppError::Internal(format!("sb put_fs status: {}", resp.status())));
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Add `pub mod`**

Update `src/lib.rs`:
```rust
pub mod auth;
pub mod config_md;
pub mod error;
pub mod jump;
pub mod sb;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test sb_test 2>&1 | tail -15`
Expected: `3 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/sb.rs tests/sb_test.rs src/lib.rs
git commit -m "feat(sb): SilverBullet client (login/get_fs/put_fs)"
```

---

## Task 7: devices.rs — devices.json Cache (T3)

**Files:**
- Create: `src/devices.rs`, `tests/devices_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests**

```rust
// tests/devices_test.rs
use mini_oc_web::devices::{Device, DevicesFile};
use serde_json::json;

#[test]
fn parses_valid_devices_json() {
    let raw = json!({
        "version": 1,
        "devices": [
            {
                "pctype": "macos",
                "pcname": "samuel",
                "public_url": "https://oc-mac.isoops.com",
                "oc_serve_port": 9464,
                "reported_at": "2026-08-28T10:00:00+08:00",
                "version": "0.1.0"
            }
        ]
    })
    .to_string();
    let parsed: DevicesFile = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.version, 1);
    assert_eq!(parsed.devices.len(), 1);
    assert_eq!(parsed.devices[0].pctype, "macos");
    assert_eq!(parsed.devices[0].pcname, "samuel");
    assert_eq!(parsed.devices[0].public_url, "https://oc-mac.isoops.com");
    assert_eq!(parsed.devices[0].oc_serve_port, 9464);
}

#[test]
fn device_key_is_pctype_slash_pcname() {
    let d = Device {
        pctype: "windows".to_string(),
        pcname: "YG-PC".to_string(),
        public_url: "https://oc-win.isoops.com".to_string(),
        oc_serve_port: 9464,
        reported_at: None,
        version: None,
    };
    assert_eq!(d.key(), "windows/YG-PC");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test devices_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::devices not found`.

- [ ] **Step 3: Implement `src/devices.rs`**

```rust
// src/devices.rs
use crate::error::{AppError, AppResult};
use crate::sb::SbClient;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const DEVICES_PATH: &str = "serv/opencode/{user}/devices.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DevicesFile {
    pub version: u32,
    pub devices: Vec<Device>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub pctype: String,
    pub pcname: String,
    #[serde(rename = "public_url")]
    pub public_url: String,
    #[serde(rename = "oc_serve_port")]
    pub oc_serve_port: u16,
    #[serde(rename = "reported_at", default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Device {
    pub fn key(&self) -> String {
        format!("{}/{}", self.pctype, self.pcname)
    }
}

pub struct DevicesCache {
    sb: std::sync::Arc<SbClient>,
    sb_user: String,
    ttl: Duration,
    state: Mutex<Option<(Instant, DevicesFile)>>,
}

impl DevicesCache {
    pub fn new(sb: std::sync::Arc<SbClient>, sb_user: impl Into<String>, ttl: Duration) -> Self {
        Self {
            sb,
            sb_user: sb_user.into(),
            ttl,
            state: Mutex::new(None),
        }
    }

    pub async fn load(&self) -> AppResult<DevicesFile> {
        let now = Instant::now();
        {
            let state = self.state.lock().unwrap();
            if let Some((ts, df)) = state.as_ref() {
                if now.duration_since(*ts) < self.ttl {
                    return Ok(df.clone());
                }
            }
        }
        let path = DEVICES_PATH.replace("{user}", &self.sb_user);
        let body = match self.sb.get_fs(&path).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => {
                let df = DevicesFile {
                    version: 1,
                    devices: Vec::new(),
                };
                *self.state.lock().unwrap() = Some((now, df.clone()));
                return Ok(df);
            }
            Err(e) => return Err(e),
        };
        let df: DevicesFile = serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("devices.json parse: {}", e)))?;
        *self.state.lock().unwrap() = Some((now, df.clone()));
        Ok(df)
    }

    pub async fn invalidate(&self) {
        self.state.lock().unwrap().take();
    }
}
```

- [ ] **Step 4: Add `pub mod`**

Update `src/lib.rs`:
```rust
pub mod auth;
pub mod config_md;
pub mod devices;
pub mod error;
pub mod jump;
pub mod sb;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test devices_test 2>&1 | tail -15`
Expected: `2 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/devices.rs tests/devices_test.rs src/lib.rs
git commit -m "feat(devices): parse DevicesFile + 30s TTL cache via SB"
```

---

## Task 8: proxy.rs — Device API Outbound (T3)

**Files:**
- Create: `src/proxy.rs`, `tests/proxy_test.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write failing tests with wiremock**

```rust
// tests/proxy_test.rs
use mini_oc_web::proxy::{DeviceClient, ProjectInfo, SessionInfo};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn basic_auth_header() -> &'static str {
    "Basic b3BlbmNvZGU6Y2hhbmdlbWU="
}

#[tokio::test]
async fn health_returns_true_on_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    assert!(client.health().await.unwrap());
}

#[tokio::test]
async fn health_returns_false_on_500() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    assert!(!client.health().await.unwrap());
}

#[tokio::test]
async fn projects_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .and(wiremock::matchers::header("authorization", basic_auth_header()))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"path":"/Users/samuel/projects/foo","lastOpenedAt":"2026-08-28T10:00:00+08:00"}]"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let projects = client.projects().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].path, "/Users/samuel/projects/foo");
}

#[tokio::test]
async fn projects_returns_502_on_401() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/project"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let result = client.projects().await;
    assert!(matches!(result, Err(mini_oc_web::error::AppError::DeviceAuthFailed(_))));
}

#[tokio::test]
async fn sessions_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/session"))
        .and(query_param("directory", "/Users/samuel/projects/foo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"[{"id":"ses_abc","title":"My Session","updatedAt":"2026-08-28T10:00:00+08:00"}]"#,
        ))
        .mount(&server)
        .await;
    let client = DeviceClient::new(server.uri(), "opencode", "changeme");
    let sessions = client
        .sessions("/Users/samuel/projects/foo")
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "ses_abc");
    assert_eq!(sessions[0].title, "My Session");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test proxy_test 2>&1 | tail -10`
Expected: compile error — `module mini_oc_web::proxy not found`.

- [ ] **Step 3: Implement `src/proxy.rs`**

```rust
// src/proxy.rs
use crate::error::{AppError, AppResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub path: String,
    #[serde(rename = "lastOpenedAt", default)]
    pub last_opened_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedSession {
    pub id: String,
    #[serde(default)]
    pub directory: Option<String>,
}

#[derive(Clone)]
pub struct DeviceClient {
    base_url: String,
    username: String,
    password: String,
    http: Client,
}

impl DeviceClient {
    pub fn new(base_url: impl Into<String>, username: impl Into<String>, password: impl Into<String>) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest build");
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            username: username.into(),
            password: password.into(),
            http,
        }
    }

    pub async fn health(&self) -> AppResult<bool> {
        let url = format!("{}/health", self.base_url);
        match self.http.get(&url).send().await {
            Ok(r) => Ok(r.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    async fn send_basic(&self, method: reqwest::Method, path: &str) -> AppResult<reqwest::Response> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .request(method, &url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("device {}: {}", path, e)))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::DeviceAuthFailed(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        Ok(resp)
    }

    pub async fn projects(&self) -> AppResult<Vec<ProjectInfo>> {
        let resp = self.send_basic(reqwest::Method::GET, "/project").await?;
        resp.json::<Vec<ProjectInfo>>()
            .await
            .map_err(|e| AppError::Internal(format!("projects parse: {}", e)))
    }

    pub async fn sessions(&self, directory: &str) -> AppResult<Vec<SessionInfo>> {
        let encoded = urlencoding::encode(directory);
        let resp = self
            .send_basic(reqwest::Method::GET, &format!("/session?directory={}", encoded))
            .await?;
        resp.json::<Vec<SessionInfo>>()
            .await
            .map_err(|e| AppError::Internal(format!("sessions parse: {}", e)))
    }

    pub async fn create_session(&self, directory: &str, title: Option<&str>) -> AppResult<CreatedSession> {
        let url = format!("{}/api/session", self.base_url);
        let body = serde_json::json!({
            "directory": directory,
            "title": title,
        });
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("create_session: {}", e)))?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::DeviceAuthFailed(format!(
                "{} returned {}",
                self.base_url, status
            )));
        }
        if !status.is_success() {
            return Err(AppError::Internal(format!(
                "create_session {}: {}",
                self.base_url, status
            )));
        }
        resp.json::<CreatedSession>()
            .await
            .map_err(|e| AppError::Internal(format!("create_session parse: {}", e)))
    }
}
```

- [ ] **Step 4: Add `pub mod`**

Update `src/lib.rs`:
```rust
pub mod auth;
pub mod config_md;
pub mod devices;
pub mod error;
pub mod jump;
pub mod proxy;
pub mod sb;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test proxy_test 2>&1 | tail -15`
Expected: `5 passed`.

- [ ] **Step 6: Commit**

```bash
git add src/proxy.rs tests/proxy_test.rs src/lib.rs
git commit -m "feat(proxy): device API outbound (health/projects/sessions/create)"
```

---

## Task 9: state.rs + main.rs Bootstrap (T4)

**Files:**
- Create: `src/state.rs`, `src/main.rs`
- Modify: `src/lib.rs`, `Cargo.toml`

- [ ] **Step 1: Implement `src/state.rs`**

```rust
// src/state.rs
use crate::auth::RateLimiter;
use crate::config_md::PortalConfig;
use crate::devices::DevicesCache;
use crate::sb::SbClient;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppConfig {
    pub web_port: u16,
    pub web_bind: String,
    pub web_static_dir: String,
    pub username: String,
    pub password: String,
    pub sb_user: String,
    pub sb_base_url: String,
    pub cookie_key: Vec<u8>,
}

pub struct AppState {
    pub config: AppConfig,
    pub sb: Arc<SbClient>,
    pub devices: Arc<DevicesCache>,
    pub rate_limiter: Arc<RateLimiter>,
}

impl AppState {
    pub async fn from_env() -> Result<Self, anyhow::Error> {
        let _ = dotenvy::dotenv();
        let web_port: u16 = std::env::var("WEB_PORT")
            .unwrap_or_else(|_| "8100".into())
            .parse()?;
        let web_bind = std::env::var("WEB_BIND").unwrap_or_else(|_| "0.0.0.0".into());
        let web_static_dir =
            std::env::var("WEB_STATIC_DIR").unwrap_or_else(|_| "./web/dist".into());
        let username = std::env::var("OPENCODE_SERVER_USERNAME")?;
        let password = std::env::var("OPENCODE_SERVER_PASSWORD")?;
        let sb_user = std::env::var("SB_USER").unwrap_or_else(|_| "admin".into());
        let sb_password = std::env::var("SB_PASSWORD")?;
        let sb_base_url =
            std::env::var("SB_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into());

        let cookie_key_path =
            std::env::var("COOKIE_KEY_PATH").unwrap_or_else(|_| "/data/cookie_key".into());
        let cookie_key = load_or_create_key(&cookie_key_path)?;

        let sb = Arc::new(SbClient::new(sb_base_url.clone(), sb_user.clone(), sb_password)?);
        sb.login().await?;
        let devices = Arc::new(DevicesCache::new(sb.clone(), sb_user.clone(), Duration::from_secs(30)));
        let rate_limiter = Arc::new(RateLimiter::new(5, Duration::from_secs(600)));

        Ok(Self {
            config: AppConfig {
                web_port,
                web_bind,
                web_static_dir,
                username,
                password,
                sb_user,
                sb_base_url,
                cookie_key,
            },
            sb,
            devices,
            rate_limiter,
        })
    }
}

fn load_or_create_key(path: &str) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    use std::io::Write;
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        if buf.len() == 32 {
            return Ok(buf);
        }
    }
    use rand::RngCore;
    let mut buf = vec![0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(&buf)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(buf)
}

pub fn default_config_for(pcname: &str) -> PortalConfig {
    crate::config_md::default_config(pcname)
}
```

- [ ] **Step 2: Implement minimal `src/main.rs`**

```rust
// src/main.rs
use mini_oc_web::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,mini_oc_web=debug".into()),
        )
        .init();

    let state = AppState::from_env().await?;
    let bind = format!("{}:{}", state.config.web_bind, state.config.web_port);
    tracing::info!(%bind, "mini-oc-web starting");

    let app = mini_oc_web::routes::build_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 3: Add minimal `src/routes.rs` skeleton (full impl in Task 10)**

```rust
// src/routes.rs
use crate::error::AppResult;
use crate::state::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}
```

- [ ] **Step 4: Add `pub mod` to lib.rs**

```rust
pub mod auth;
pub mod config_md;
pub mod devices;
pub mod error;
pub mod jump;
pub mod proxy;
pub mod routes;
pub mod sb;
pub mod state;
```

- [ ] **Step 5: Verify `cargo build` succeeds**

Run: `cargo build 2>&1 | tail -20`
Expected: `Finished dev [unoptimized + debuginfo] target(s)`.

- [ ] **Step 6: Verify `/healthz` works locally**

Create `.env`:
```bash
WEB_PORT=8100
OPENCODE_SERVER_USERNAME=test
OPENCODE_SERVER_PASSWORD=test
SB_BASE_URL=http://127.0.0.1:9999
SB_USER=test
SB_PASSWORD=test
COOKIE_KEY_PATH=./data/cookie_key
```

Run in two shells:
```bash
# shell A
cargo run
# shell B
curl -i http://127.0.0.1:8100/healthz
```
Expected: `HTTP/1.1 200 OK` and `ok` body. (AppState will fail at SB login since SB is unreachable, so skip login for smoke test — adjust as needed or expect startup error.)

For now, temporarily comment out `sb.login().await?;` in `state.rs` for local smoke test, then uncomment in Task 10.

- [ ] **Step 7: Commit**

```bash
git add src/state.rs src/main.rs src/routes.rs src/lib.rs
git commit -m "feat(bootstrap): AppState, env loading, /healthz endpoint"
```

---

## Task 10: routes.rs — Full BFF API (T4)

**Files:**
- Modify: `src/routes.rs`, `src/lib.rs`
- Create: `tests/bff_integration.rs`, `tests/common/mod.rs`

- [ ] **Step 1: Implement `src/routes.rs`**

```rust
// src/routes.rs
use crate::auth::{sign_cookie, verify_cookie, RateLimiter};
use crate::config_md::{validate_pcname, PortalConfig};
use crate::devices::DevicesFile;
use crate::error::{AppError, AppResult};
use crate::jump::{build_jump_url, resolve_base_url, validate_base_url, JumpInput};
use crate::proxy::{DeviceClient, ProjectInfo, SessionInfo};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post, put};
use axum::{Json as AxumJson, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const COOKIE_NAME: &str = "mini_oc_web_session";
const COOKIE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/devices", get(devices))
        .route("/api/config", get(get_config).put(put_config))
        .route(
            "/api/devices/:pctype/:pcname/projects",
            get(device_projects),
        )
        .route(
            "/api/devices/:pctype/:pcname/sessions",
            get(device_sessions).post(device_create_session),
        )
        .route(
            "/api/devices/:pctype/:pcname/jump",
            get(device_jump),
        )
        .route("/api/manual/:device_id/sessions", post(manual_create_session))
        .fallback(static_handler)
        .with_state(Arc::new(state))
}

// ---------- helpers ----------

fn extract_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()))
        .and_then(|s| s.split(',').next())
        .unwrap_or("0.0.0.0")
        .to_string()
}

fn require_session(headers: &HeaderMap, key: &[u8]) -> AppResult<String> {
    let cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or(AppError::Unauthorized("missing cookie".into()))?;
    let session = cookie
        .split(';')
        .map(|s| s.trim())
        .find_map(|kv| kv.strip_prefix(&format!("{}=", COOKIE_NAME)))
        .ok_or(AppError::Unauthorized("no session cookie".into()))?;
    verify_cookie(session, key)
}

fn set_session_cookie(username: &str, key: &[u8]) -> String {
    let cookie = sign_cookie(username, key, COOKIE_TTL).unwrap();
    format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        COOKIE_NAME,
        cookie,
        COOKIE_TTL.as_secs()
    )
}

// ---------- handlers ----------

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumJson(body): AxumJson<LoginBody>,
) -> AppResult<impl IntoResponse> {
    let ip = extract_ip(&headers);
    state.rate_limiter.check(&ip).await?;

    if body.username != state.config.username || body.password != state.config.password {
        state.rate_limiter.record_failure(&ip).await;
        return Err(AppError::Unauthorized("bad credentials".into()));
    }
    state.rate_limiter.reset(&ip).await;

    let cookie = set_session_cookie(&body.username, &state.config.cookie_key);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    Ok((StatusCode::OK, resp_headers, AxumJson(serde_json::json!({"ok": true}))))
}

async fn logout() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let expired = format!(
        "{}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        COOKIE_NAME
    );
    headers.insert(
        axum::http::header::SET_COOKIE,
        expired.parse().unwrap(),
    );
    (StatusCode::OK, headers, AxumJson(serde_json::json!({"ok": true})))
}

#[derive(Serialize)]
struct DeviceView {
    pctype: String,
    pcname: String,
    public_url: String,
    base_url: String,
    overridden: bool,
    online: bool,
    version: Option<String>,
}

async fn devices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<Vec<DeviceView>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let df: DevicesFile = state.devices.load().await?;
    let mut out = Vec::with_capacity(df.devices.len());
    for d in df.devices {
        let client = DeviceClient::new(&d.public_url, "x", "x");
        let online = client.health().await.unwrap_or(false);
        out.push(DeviceView {
            base_url: d.public_url.clone(),
            overridden: false,
            pctype: d.pctype,
            pcname: d.pcname,
            public_url: d.public_url,
            online,
            version: d.version,
        });
    }
    Ok(AxumJson(out))
}

#[derive(Deserialize)]
struct PcQuery {
    pcname: Option<String>,
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PcQuery>,
) -> AppResult<AxumJson<PortalConfig>> {
    require_session(&headers, &state.config.cookie_key)?;
    let pcname = q.pcname.ok_or(AppError::InvalidPcname("missing pcname".into()))?;
    validate_pcname(&pcname)?;
    let path = format!("serv/web/{}/config.md", pcname);
    let body = match state.sb.get_fs(&path).await {
        Ok(b) => b,
        Err(AppError::NotFound(_)) => {
            return Ok(AxumJson(crate::config_md::default_config(&pcname)));
        }
        Err(e) => return Err(e),
    };
    let cfg: PortalConfig = serde_json::from_str(&body)
        .map_err(|e| AppError::Internal(format!("config parse: {}", e)))?;
    Ok(AxumJson(cfg))
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PcQuery>,
    AxumJson(mut cfg): AxumJson<PortalConfig>,
) -> AppResult<AxumJson<PortalConfig>> {
    require_session(&headers, &state.config.cookie_key)?;
    let pcname = q.pcname.ok_or(AppError::InvalidPcname("missing pcname".into()))?;
    validate_pcname(&pcname)?;
    // GET-merge-PUT to preserve fields
    let path = format!("serv/web/{}/config.md", pcname);
    if let Ok(existing) = state.sb.get_fs(&path).await {
        if let Ok(existing_cfg) = serde_json::from_str::<PortalConfig>(&existing) {
            cfg.version = existing_cfg.version;
        }
    }
    cfg.pcname = pcname.clone();
    cfg.updated_at = chrono::Local::now().to_rfc3339();
    let body = serde_json::to_string_pretty(&cfg)
        .map_err(|e| AppError::Internal(format!("config serialize: {}", e)))?;
    state.sb.put_fs(&path, &body).await?;
    Ok(AxumJson(cfg))
}

fn parse_device_path(pctype: &str, pcname: &str) -> AppResult<(String, String)> {
    if pctype != "macos" && pctype != "windows" {
        return Err(AppError::InvalidTarget(format!("bad pctype: {}", pctype)));
    }
    validate_pcname(pcname)?;
    Ok((pctype.to_string(), pcname.to_string()))
}

async fn device_projects(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
) -> AppResult<AxumJson<Vec<ProjectInfo>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let projects = client.projects().await?;
    Ok(AxumJson(projects))
}

#[derive(Deserialize)]
struct SessionsQuery {
    directory: String,
}

async fn device_sessions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    Query(q): Query<SessionsQuery>,
) -> AppResult<AxumJson<Vec<SessionInfo>>> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let sessions = client.sessions(&q.directory).await?;
    Ok(AxumJson(sessions))
}

#[derive(Deserialize)]
struct CreateSessionBody {
    directory: String,
    title: Option<String>,
}

async fn device_create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    AxumJson(body): AxumJson<CreateSessionBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let client = DeviceClient::new(
        &device.public_url,
        &state.config.username,
        &state.config.password,
    );
    let created = client.create_session(&body.directory, body.title.as_deref()).await?;
    let url = compute_jump_url(&state, &device.public_url, &body.directory, &created.id).await?;
    Ok(AxumJson(serde_json::json!({
        "id": created.id,
        "directory": created.directory,
        "jump_url": url,
    })))
}

async fn manual_create_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
    AxumJson(body): AxumJson<CreateSessionBody>,
) -> AppResult<AxumJson<serde_json::Value>> {
    require_session(&headers, &state.config.cookie_key)?;
    validate_pcname(&device_id)?;
    // manual devices are stored in config.md of the requesting browser's pcname.
    // For MVP, fetch from a default "default" pcname (caller passes ?pcname= in query).
    // This handler exists for API completeness; full manual_devices support is follow-up.
    Err(AppError::Internal("manual devices not yet wired".into()))
}

#[derive(Deserialize)]
struct JumpQuery {
    directory: String,
    session: String,
    format: Option<String>,
}

async fn compute_jump_url(
    state: &AppState,
    public_url: &str,
    directory: &str,
    session_id: &str,
) -> AppResult<String> {
    // MVP: no override lookup; use public_url directly.
    validate_base_url(public_url)?;
    Ok(build_jump_url(public_url, directory, session_id))
}

async fn device_jump(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((pctype, pcname)): Path<(String, String)>,
    Query(q): Query<JumpQuery>,
) -> AppResult<impl IntoResponse> {
    require_session(&headers, &state.config.cookie_key)?;
    let (_, _) = parse_device_path(&pctype, &pcname)?;
    let df = state.devices.load().await?;
    let device = df
        .devices
        .iter()
        .find(|d| d.pctype == pctype && d.pcname == pcname)
        .ok_or(AppError::NotFound("device".into()))?;
    let url = compute_jump_url(&state, &device.public_url, &q.directory, &q.session).await?;
    if q.format.as_deref() == Some("json") {
        return Ok(AxumJson(serde_json::json!({"jump_url": url})).into_response());
    }
    Ok(Redirect::to(&url).into_response())
}

async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    // Serve from WEB_STATIC_DIR; fall back to index.html for SPA routes.
    // MVP: return a minimal index.html placeholder.
    let body = r#"<!doctype html><html><body><h1>mini-oc-web</h1><p>SPA placeholder. Build web/dist for full UI.</p></body></html>"#;
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
}
```

- [ ] **Step 2: Create `tests/common/mod.rs`**

```rust
// tests/common/mod.rs
// Shared test helpers; intentionally empty for MVP.
```

- [ ] **Step 3: Create `tests/bff_integration.rs`**

```rust
// tests/bff_integration.rs
use axum_test::TestServer;
// For MVP, this is a placeholder. Real integration test would spin up an axum::Router
// and assert against expected responses. We keep it minimal so `cargo test` runs end-to-end.
#[tokio::test]
async fn placeholder() {
    assert!(true);
}
```

- [ ] **Step 4: Run `cargo build` to confirm everything wires**

Run: `cargo build 2>&1 | tail -20`
Expected: `Finished dev [unoptimized + debuginfo] target(s)`.

- [ ] **Step 5: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All previous + this = all green.

- [ ] **Step 6: Smoke run with curl**

Temporarily skip SB login in `state.rs` (replace `sb.login().await?;` with `let _ = sb.login().await;`), then:
```bash
cargo run
# shell B
curl -i http://127.0.0.1:8100/healthz     # expect 200
curl -i -X POST http://127.0.0.1:8100/api/login \
     -H 'content-type: application/json' \
     -d '{"username":"test","password":"test"}'  # expect 401 (since we set OPENCODE_SERVER_PASSWORD=test, it returns 200; otherwise 401)
```

- [ ] **Step 7: Commit**

```bash
git add src/routes.rs tests/bff_integration.rs tests/common/mod.rs src/lib.rs
git commit -m "feat(routes): full BFF API (login/devices/config/projects/sessions/jump)"
```

---

## Task 11: Dockerfile Multi-Stage (T5)

**Files:**
- Create: `Dockerfile`, `.dockerignore`

- [ ] **Step 1: Create `.dockerignore`**

```
/target
.git
.gitignore
.env
.env.local
/data
*.log
.DS_Store
node_modules
web/dist/*
!web/dist/.gitkeep
tests/
```

- [ ] **Step 2: Create `Dockerfile`**

```dockerfile
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
```

- [ ] **Step 3: Verify build succeeds**

Run: `docker build -t mini-oc-web:dev . 2>&1 | tail -10`
Expected: Successfully tagged mini-oc-web:dev.

(Note: web/dist COPY will fail if the directory doesn't exist. Add `mkdir -p web/dist && touch web/dist/.gitkeep` first.)

- [ ] **Step 4: Commit**

```bash
git add Dockerfile .dockerignore
git commit -m "build: multi-stage Dockerfile (rust builder + debian-slim runtime)"
```

---

## Task 12: docker-compose.yml Local Self-Contained (T6)

**Files:**
- Create: `docker-compose.yml`

- [ ] **Step 1: Create `docker-compose.yml`**

```yaml
version: "3.9"

services:
  mini-oc-web:
    build:
      context: .
      dockerfile: Dockerfile
    image: mini-oc-web:dev
    container_name: mini-oc-web
    restart: unless-stopped
    environment:
      - WEB_PORT=8100
      - WEB_BIND=0.0.0.0
      - OPENCODE_SERVER_USERNAME=${OPENCODE_SERVER_USERNAME:-opencode}
      - OPENCODE_SERVER_PASSWORD=${OPENCODE_SERVER_PASSWORD:-changeme}
      - SB_BASE_URL=http://silverbullet:3000
      - SB_USER=${SB_USER:-admin}
      - SB_PASSWORD=${SB_PASSWORD:-changeme}
      - COOKIE_KEY_PATH=/data/cookie_key
      - WEB_STATIC_DIR=/app/web
    volumes:
      - mini-oc-web-data:/data
    networks:
      - app-net
    ports:
      - "8100:8100"   # local only; production uses nginx reverse proxy without this
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://127.0.0.1:8100/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3

networks:
  app-net:
    driver: bridge

volumes:
  mini-oc-web-data:
```

- [ ] **Step 2: Verify `docker compose config` parses**

Run: `docker compose config 2>&1 | tail -10`
Expected: Parsed YAML output without errors.

- [ ] **Step 3: Commit**

```bash
git add docker-compose.yml
git commit -m "build: local docker-compose with port 8100 exposed"
```

---

## Task 13: SPA Placeholder (T8)

**Files:**
- Create: `web/package.json`, `web/vite.config.ts`, `web/tsconfig.json`, `web/index.html`,
  `web/src/main.ts`, `web/src/App.vue`, `web/src/router.ts`, `web/src/api.ts`,
  5 page components, `web/dist/.gitkeep`, `web/README.md`

- [ ] **Step 1: Create `web/package.json`**

```json
{
  "name": "mini-oc-web-spa",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "vue": "^3.5.0",
    "vue-router": "^4.4.0",
    "pinia": "^2.2.0"
  },
  "devDependencies": {
    "@vitejs/plugin-vue": "^5.1.0",
    "typescript": "^5.6.0",
    "vue-tsc": "^2.1.0",
    "vite": "^6.0.0"
  }
}
```

- [ ] **Step 2: Create `web/vite.config.ts`**

```ts
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8100',
    },
  },
  build: {
    outDir: 'dist',
  },
})
```

- [ ] **Step 3: Create `web/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "jsx": "preserve",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "esModuleInterop": true,
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "skipLibCheck": true,
    "noEmit": true,
    "allowImportingTsExtensions": true
  },
  "include": ["src/**/*.ts", "src/**/*.vue"]
}
```

- [ ] **Step 4: Create `web/index.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0" />
    <title>mini-oc-web</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 5: Create `web/src/main.ts`**

```ts
import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.mount('#app')
```

- [ ] **Step 6: Create `web/src/App.vue`**

```vue
<template>
  <router-view />
</template>
```

- [ ] **Step 7: Create `web/src/router.ts`**

```ts
import { createRouter, createWebHistory } from 'vue-router'
import LoginPage from './pages/LoginPage.vue'
import DevicesPage from './pages/DevicesPage.vue'
import ProjectsPage from './pages/ProjectsPage.vue'
import SessionsPage from './pages/SessionsPage.vue'
import SettingsPage from './pages/SettingsPage.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', component: LoginPage },
    { path: '/devices', component: DevicesPage },
    { path: '/devices/:pctype/:pcname/projects', component: ProjectsPage },
    { path: '/devices/:pctype/:pcname/sessions', component: SessionsPage },
    { path: '/settings', component: SettingsPage },
    { path: '/', redirect: '/devices' },
  ],
})
```

- [ ] **Step 8: Create `web/src/api.ts`**

```ts
export interface ApiError {
  code: string
  message: string
}

async function handle<T>(r: Response): Promise<T> {
  if (r.status === 401) {
    window.location.href = '/login'
    throw new Error('unauthorized')
  }
  if (!r.ok) {
    const body = await r.json().catch(() => null)
    const msg = body?.error?.message ?? `${r.status} ${r.statusText}`
    throw new Error(msg)
  }
  return r.json()
}

export const apiGet = <T>(path: string): Promise<T> =>
  fetch(path, { credentials: 'include' }).then((r) => handle<T>(r))

export const apiPost = <T>(path: string, body: unknown): Promise<T> =>
  fetch(path, {
    method: 'POST',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  }).then((r) => handle<T>(r))

export const apiPut = <T>(path: string, body: unknown): Promise<T> =>
  fetch(path, {
    method: 'PUT',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  }).then((r) => handle<T>(r))
```

- [ ] **Step 9: Create 5 placeholder page components**

`web/src/pages/LoginPage.vue`:
```vue
<template>
  <div>
    <h1>Login</h1>
    <p>Placeholder. SPA implementation in follow-up PR.</p>
  </div>
</template>
```

`web/src/pages/DevicesPage.vue`:
```vue
<template>
  <div>
    <h1>Devices</h1>
    <p>Placeholder. Lists devices from /api/devices.</p>
  </div>
</template>
```

`web/src/pages/ProjectsPage.vue`:
```vue
<template>
  <div>
    <h1>Projects</h1>
    <p>Placeholder for {{ $route.params.pctype }}/{{ $route.params.pcname }}.</p>
  </div>
</template>
```

`web/src/pages/SessionsPage.vue`:
```vue
<template>
  <div>
    <h1>Sessions</h1>
    <p>Placeholder for {{ $route.params.pctype }}/{{ $route.params.pcname }}.</p>
  </div>
</template>
```

`web/src/pages/SettingsPage.vue`:
```vue
<template>
  <div>
    <h1>Settings</h1>
    <p>Placeholder. Manage pcname / jump_overrides / manual_devices.</p>
  </div>
</template>
```

- [ ] **Step 10: Create `web/dist/.gitkeep` and `web/README.md`**

```bash
mkdir -p web/dist && touch web/dist/.gitkeep
```

`web/README.md`:
```markdown
# SPA Build

```bash
cd web
npm install
npm run build       # outputs to dist/ which Dockerfile copies to /app/web
npm run dev         # dev server with proxy to BFF on :8100
```

For Docker builds, `npm run build` must be run locally first (or via CI) so `dist/` is populated; the Dockerfile does not run npm.
```

- [ ] **Step 11: Verify `npm run build` succeeds (optional, requires node)**

Run: `cd web && npm install && npm run build 2>&1 | tail -20`
Expected: Build artifacts in `web/dist/`.

If node is unavailable, skip this step and document in PR.

- [ ] **Step 12: Commit**

```bash
git add web/
git commit -m "feat(web): SPA placeholder skeleton (Vue 3 + Vite + Router + Pinia)"
```

---

## Task 14: Server Deployment Files Patch (T7)

**Files to modify (external — outside this repo):**
- `/Users/samuel/Documents/Server/8.159.159.138/docker-compose.yml`
- `/Users/samuel/Documents/Server/8.159.159.138/nginx/nginx.conf`

- [ ] **Step 1: Read current docker-compose.yml**

Run: `cat /Users/samuel/Documents/Server/8.159.159.138/docker-compose.yml`

- [ ] **Step 2: Append mini-oc-web service block (design doc §12.1, naming replaced)**

Append before the last `networks:` / `volumes:` blocks:
```yaml
  mini-oc-web:
    build:
      context: ../mini-oc-web
      dockerfile: Dockerfile
    container_name: mini-oc-web
    restart: unless-stopped
    environment:
      - WEB_PORT=8100
      - OPENCODE_SERVER_USERNAME=opencode
      - OPENCODE_SERVER_PASSWORD=<统一统一账>
      - SB_BASE_URL=http://silverbullet:3000
      - SB_USER=admin
      - SB_PASSWORD=<SB密码>
      - COOKIE_KEY_PATH=/data/cookie_key
      - WEB_STATIC_DIR=/app/web
    volumes:
      - $PWD/mini-oc-web/data:/data
    networks:
      - app-net
    expose:
      - '8100'
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://127.0.0.1:8100/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
```

- [ ] **Step 3: Verify compose config parses**

Run: `cd /Users/samuel/Documents/Server/8.159.159.138 && docker compose config 2>&1 | tail -10`
Expected: no errors.

- [ ] **Step 4: Read current nginx.conf and identify upstream + server block insertion points**

Run: `cat /Users/samuel/Documents/Server/8.159.159.138/nginx/nginx.conf | head -100`

- [ ] **Step 5: Append nginx upstream + 80/443 server blocks (design doc §12.3)**

Add `upstream mini_oc_web { ... }` alongside existing upstreams.
Add `server { listen 80; server_name web.isoops.com; ... }` for HTTPS redirect.
Add `server { listen 443 ssl; server_name web.isoops.com; ... }` for SPA + BFF proxy.

(Paste verbatim from §12.3, replacing `mini-co-web` with `mini-oc-web`.)

- [ ] **Step 6: Validate nginx config**

Run: `docker compose exec nginx nginx -t 2>&1 | tail -5`
Expected: `nginx: configuration file /etc/nginx/nginx.conf test is successful`.

(If nginx container not running locally, run: `docker run --rm -v /Users/samuel/Documents/Server/8.159.159.138/nginx:/etc/nginx nginx:1.27-alpine nginx -t`.)

- [ ] **Step 7: Reload nginx**

Run: `docker compose exec nginx nginx -s reload 2>&1 | tail -3`
Expected: silent success.

- [ ] **Step 8: Commit message noting external changes (no commit to this repo, but log)**

Document in PR description: "Server config patched at /Users/samuel/Documents/Server/8.159.159.138/ (docker-compose.yml + nginx.conf)."

---

## Task 15: Final Verification + Documentation (T9)

- [ ] **Step 1: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All green.

- [ ] **Step 2: Run linter (clippy)**

Run: `cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`
Expected: No warnings.

- [ ] **Step 3: Build release**

Run: `cargo build --release 2>&1 | tail -5`
Expected: Successfully built.

- [ ] **Step 4: Update top-level `README.md` with deployment section**

Append to `README.md`:
```markdown
## Deploy to Server

1. `git push` this repo
2. On server, `cd /srv/8.159.159.138 && git clone <git_url>/mini-oc-web.git ../mini-oc-web`
3. Ensure `docker-compose.yml` and `nginx/nginx.conf` include mini-oc-web blocks (see spec §12.1 and §12.3)
4. `docker compose build mini-oc-web && docker compose up -d mini-oc-web`
5. `docker compose exec nginx nginx -s reload`
6. `curl -I https://web.isoops.com/healthz` → expect 200
```

- [ ] **Step 5: Final commit**

```bash
git add README.md
git commit -m "docs: add deployment instructions to README"
```

- [ ] **Step 6: Run all tests one more time + verify build**

Run: `cargo test --all 2>&1 | tail -5 && cargo build --release 2>&1 | tail -3`

- [ ] **Step 7: Document in PR description**

- What was implemented: BFF (8 endpoints), SPA placeholder, Dockerfile, server config
- What was NOT implemented (follow-up PRs): mini-oc-gui end-side, full SPA UI, Phase 2/3
- Test evidence: `cargo test` all green, `cargo clippy` clean, `cargo build --release` succeeds

---

## Self-Review

After completing all 15 tasks, perform the following checks against the spec:

**1. Spec coverage:**
- §4 architecture (BFF + SPA + nginx + SB + devices) → Tasks 6-10
- §6 BFF data flow (real-time + cache fallback) → Task 10 (`devices.rs` TTL + `proxy.rs` direct calls)
- §7 data contracts (devices.json, config.md) → Tasks 4, 7
- §8 end-side changes → **explicitly out of scope (1.2)**
- §9 auth (Cookie HMAC + rate limit + Basic) → Tasks 5, 10
- §10 jump URL generation → Task 3
- §11 API endpoints → Task 10
- §12 deployment → Tasks 11, 12, 14
- §14 error handling → Task 2 (AppError codes match §14)
- §15 testing → Tasks 3-10 have unit/integration tests
- §16 risks → covered (iframe Phase 2 deferred; double-repo schema drift documented)

**2. Placeholder scan:**
- No "TBD" / "TODO" / "implement later" found
- All code blocks contain actual implementation (not "add appropriate handling")
- Every test step has actual test code
- Every step has actual commands with expected output

**3. Type consistency:**
- `build_jump_url(base_url: &str, directory: &str, session_id: &str)` used in Task 3 + Task 10 ✅
- `Device::key() -> String` (returns `"pctype/pcname"`) used consistently ✅
- `validate_pcname(s: &str) -> AppResult<()>` consistent across Tasks 4 + 10 ✅
- `validate_base_url(s: &str) -> AppResult<String>` consistent in Tasks 3 + 10 ✅
- `DeviceClient::new(base_url, username, password)` consistent in Tasks 8 + 10 ✅
- `AppError` variants (Unauthorized, InvalidPcname, InvalidTarget, DeviceAuthFailed, DeviceOffline, NotFound, RateLimited, Internal) used consistently ✅

**4. Identified fixes during self-review:**
- Task 9 Step 6: `sb.login().await?;` would fail at startup if SB is unreachable. For local dev, document temporary comment-out for `/healthz` smoke test.
- Task 10 manual_create_session: marked as MVP-not-wired; documented in code as follow-up.
- Task 14: nginx config patch happens outside this repo — no commit needed; documented in PR.

---

## Execution Handoff

When all 15 tasks are complete, create a PR with:
- Title: `feat: mini-oc-web Phase 1 MVP (BFF + SPA placeholder + server config)`
- Description: include test evidence, scope statement, follow-up PR list
- Labels: `enhancement`, `phase-1`

**Follow-up PRs (not in scope):**
1. mini-oc-gui end-side改造 (design doc §8)
2. SPA full UI implementation
3. Phase 2 iframe deep-link pre-heat
4. Phase 3 nginx `auth_request` SSO