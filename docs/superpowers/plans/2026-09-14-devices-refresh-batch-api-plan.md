# DevicesPage 刷新架构重构：`/api/me` 批量返运行时 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把「DevicesPage 刷新设备状态」从「1× `/api/me` + N× `/api/device-status/:port`」改为「1× `/api/me`（含运行时）」，并去除 `UsersCache` / `DevicesCache` 的 30 秒 TTL。

**Architecture:** BFF 删除两个缓存 wrapper（`UsersCache` / `DevicesCache`），改为直接持有 `SbClient` 的薄 store；`/api/me` handler 内部对用户每台设备并发探测（`futures::future::join_all`），把 `online` / `status` 字段直接写进响应的 `devices[i]`。SPA `DevicesPage` 删除 `statuses` ref 和 N 次 `device-status` 调用，改为读 `auth.me.devices[i]` 上的运行时字段。

**Tech Stack:** Rust (axum 0.7 + tokio + reqwest) · Vue 3 (Composition API + `<script setup>`) · TypeScript · `wiremock` + `serial_test` (Rust 集成测试) · `vue-tsc` + `vite build` (SPA 自动化门禁)

**Spec:** [`../specs/2026-09-14-devices-refresh-batch-api-design.md`](../specs/2026-09-14-devices-refresh-batch-api-design.md)（commit `0585934`）

---

## File Structure

| File                                          | Responsibility                                                                                  |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `src/users.rs`                                  | `UsersStore` (新)：直接 `SbClient` 持有，提供 `load()` / `save()`，无 TTL                       |
| `src/devices.rs`                               | `DevicesStore` (新)：同上；新增 `probe_device_status()` helper + `DeviceProbeResult` 类型         |
| `src/state.rs`                                 | `AppState.users` / `.devices` 改为 `Arc<UsersStore>` / `Arc<DevicesStore>`                       |
| `src/routes.rs` `me` handler                  | 改写：load users → 并发 probe → enrich devices → 返回                                              |
| `src/routes.rs` 其他 load 调用点              | `state.users.load()` / `state.devices.load()` 适配新类型签名（同名，无缓存）                       |
| `web/src/api.ts`                                | `UserDevice` 增加可选 `online` / `reason` / `status` 字段                                          |
| `web/src/pages/DevicesPage.vue`                | 大幅简化 refreshAll / silentFirstLoad / helper / dotClass；删除 `statuses` ref                     |
| `tests/bff_integration.rs`                      | 更新 `me_returns_*` 断言新结构；新增「no cache staleness」测试                                    |

---

## Task 1: BFF `UsersStore` 替换 `UsersCache`

**Files:**
- Modify: `src/users.rs:399-482`（替换 `UsersCache` 整个 impl + struct → `UsersStore`）
- Modify: `src/state.rs:4,68,106-107,131`（类型别名 + 构造调用）

- [ ] **Step 1: 写失败测试 — `users_store_load_hits_sb_every_call`**

在 `src/users.rs` 末尾的 `#[cfg(test)] mod tests` 内新增一个集成测试（**注意**：这是模块级测试，不需要 axum/wiremock，直接构造 `SbClient` + `UsersStore`，mock SB 用 `wiremock`）：

```rust
#[cfg(test)]
mod store_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn users_store_load_hits_sb_every_call() {
        let sb = MockServer::start().await;
        // Expect TWO GET calls (proves no cache short-circuits the second)
        Mock::given(method("GET"))
            .and(path("/.fs/web/opencode/config.md"))
            .respond_with(ResponseTemplate::new(200).set_body_string(USERS_FILE_BODY))
            .expect(2)
            .mount(&sb)
            .await;
        let store = UsersStore::new(std::sync::Arc::new(
            crate::sb::SbClient::new(sb.uri(), "admin".into(), "pw".into()).unwrap(),
        ));
        // First call
        let _ = store.load().await.unwrap();
        // Second call — must hit SB again (no cache)
        let _ = store.load().await.unwrap();
    }

    const USERS_FILE_BODY: &str = r#"{"users":[]}"#;
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run:
```bash
cargo test --lib users_store_load_hits_sb_every_call
```
Expected: **FAIL** — `UsersStore` not defined yet (compile error).

- [ ] **Step 3: 实现 `UsersStore`（替换 `UsersCache` struct + impl + helper fetch）**

**Delete** `src/users.rs:399-482`（entire `UsersCache` struct + impl including `load`, `load_fresh`, `fetch`, `fetch_and_migrate`, `invalidate` methods).

**Replace** with:

```rust
/// Thin store wrapping `SbClient` directly — no in-process TTL. Each call
/// to `load()` hits SB fresh. Rationale (ADR #20): small user base,
/// infrequent clicks, SB can absorb the load; the cache's stale window
/// outweighed its perf benefit.
pub struct UsersStore {
    sb: std::sync::Arc<SbClient>,
    /// Kept so `save()` can re-write to `web/opencode/config.md` (admin
    /// registry file location is keyed by sb_user, not portal user).
    sb_user: String,
}

impl UsersStore {
    pub fn new(sb: std::sync::Arc<SbClient>, sb_user: impl Into<String>) -> Self {
        Self {
            sb,
            sb_user: sb_user.into(),
        }
    }

    /// Load `web/opencode/config.md`. A missing document is an empty
    /// registry (first boot — the admin creates the first user). Always
    /// hits SB; no cache.
    pub async fn load(&self) -> AppResult<UsersFile> {
        let body = match self.sb.get_fs(USERS_PATH).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => return Ok(UsersFile::default()),
            Err(e) => return Err(e),
        };
        let mut uf: UsersFile = serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("users config parse: {}", e)))?;
        if migrate_legacy_ids(&mut uf) {
            let body = serialize_users_file(&uf)?;
            self.sb.put_fs(USERS_PATH, &body).await?;
        }
        Ok(uf)
    }

    pub async fn save(&self, file: &UsersFile) -> AppResult<()> {
        let body = serialize_users_file(file)?;
        self.sb.put_fs(USERS_PATH, &body).await?;
        Ok(())
    }
}
```

Notes:
- `migrate_legacy_ids` and `serialize_users_file` are private helpers in the same file — keep them, just relocate callers.
- `UsersFile::default()` is the existing `Default` impl — should already exist (verify; if not, add `#[derive(Default)]` to `UsersFile` in this file).

- [ ] **Step 4: 更新 `src/state.rs` 类型引用**

Three small edits in `src/state.rs`:

1. Line 4: `use crate::users::UsersCache;` → `use crate::users::UsersStore;`
2. Line 69: `pub users: Arc<UsersCache>,` → `pub users: Arc<UsersStore>,`
3. Line 107: `let users = Arc::new(UsersCache::new(sb.clone(), Duration::from_secs(30)));` → `let users = Arc::new(UsersStore::new(sb.clone(), sb_user.clone()));`

Also update the import in `src/state.rs:4` and remove unused `Duration` import if no other consumer needs it (it likely does — `cookie_key` uses `Instant`, etc. — leave it).

- [ ] **Step 5: 运行测试，确认通过**

Run:
```bash
cargo test --lib users_store_load_hits_sb_every_call
```
Expected: **PASS** (MockServer's `.expect(2)` enforces two HTTP hits).

- [ ] **Step 6: 编译整库（暴露其他调用点编译错误）**

Run:
```bash
cargo build --lib
```
Expected: **WILL FAIL** — many call sites still use `UsersCache`. That's expected; we'll fix them in Task 4. Do NOT yet commit; Task 4 will batch the call-site updates into one commit.

- [ ] **Step 7: 暂存 Task 1 改动**

Do NOT commit yet. Use `git add -p` or stash so Task 4 can amend the commit with the call-site updates:
```bash
git add -A
git stash push -m "wip: Task 1 UsersStore" --keep-index
```
(Or just keep the working tree dirty until Task 4 commits everything together — simpler.)

---

## Task 2: BFF `DevicesStore` 替换 `DevicesCache` + `probe_device_status` helper

**Files:**
- Modify: `src/devices.rs:71-126`（替换 `DevicesCache` 整个 impl + struct → `DevicesStore` + 新增 `probe_device_status`）

- [ ] **Step 1: 写失败测试 — `devices_store_load_hits_sb_every_call` + `probe_device_status_returns_offline_when_unreachable`**

Append to `src/devices.rs` `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod store_tests {
    use super::*;
    use std::net::SocketAddr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn devices_store_load_hits_sb_every_call() {
        let sb = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.fs/serv/opencode/admin/devices.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"version":1,"devices":[]}"#))
            .expect(2)
            .mount(&sb)
            .await;
        let store = DevicesStore::new(
            std::sync::Arc::new(crate::sb::SbClient::new(sb.uri(), "admin".into(), "pw".into()).unwrap()),
            "admin",
        );
        let _ = store.load().await.unwrap();
        let _ = store.load().await.unwrap();
    }

    #[tokio::test]
    async fn probe_device_status_returns_offline_when_unreachable() {
        // Probe a port nothing listens on → must return online=false within ~5s
        let result = probe_device_status("127.0.0.1", 1).await; // port 1 = unreachable
        assert!(!result.online, "expected offline, got: {:?}", result);
        assert!(result.reason.is_some());
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run:
```bash
cargo test --lib --no-fail-fast devices_store_load_hits_sb_every_call probe_device_status_returns_offline_when_unreachable
```
Expected: **FAIL** — `DevicesStore` / `probe_device_status` not defined.

- [ ] **Step 3: 实现 `DevicesStore` + `probe_device_status` + `DeviceProbeResult`**

**Delete** `src/devices.rs:71-126` (`DevicesCache` struct + impl + `invalidate`).

**Replace** with:

```rust
/// Result of probing one device's `GET /status` endpoint via the user's
/// cloud tunnel. Never throws — failures are encoded as
/// `online=false` with a `reason` string.
#[derive(Debug, Clone)]
pub struct DeviceProbeResult {
    pub online: bool,
    pub reason: Option<String>,
    pub status: Option<serde_json::Value>,
}

/// Probe `{cloud_ip}:{port}/status` with a 4s per-request timeout. Empty
/// credentials — the BFF→device health probe is credential-free by
/// design (ADR #13). Any failure (connection refused, timeout, non-2xx,
/// body parse error) is captured as `online=false` with a `reason`.
pub async fn probe_device_status(cloud_ip: &str, port: u16) -> DeviceProbeResult {
    let url = format!("http://{cloud_ip}:{port}/status");
    let http = match reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return DeviceProbeResult {
                online: false,
                reason: Some(format!("client build: {e}")),
                status: None,
            }
        }
    };
    match http.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(s) => DeviceProbeResult { online: true, reason: None, status: Some(s) },
            Err(e) => DeviceProbeResult {
                online: false,
                reason: Some(format!("body parse: {e}")),
                status: None,
            },
        },
        Ok(resp) => DeviceProbeResult {
            online: false,
            reason: Some(format!("device returned HTTP {}", resp.status())),
            status: None,
        },
        Err(e) => DeviceProbeResult {
            online: false,
            reason: Some(e.to_string()),
            status: None,
        },
    }
}

/// Thin store wrapping `SbClient` directly — no in-process TTL. Each
/// `load()` hits SB fresh (ADR #21).
pub struct DevicesStore {
    sb: std::sync::Arc<SbClient>,
    sb_user: String,
}

impl DevicesStore {
    pub fn new(sb: std::sync::Arc<SbClient>, sb_user: impl Into<String>) -> Self {
        Self {
            sb,
            sb_user: sb_user.into(),
        }
    }

    /// Load `serv/opencode/{user}/devices.json`. Missing file → empty
    /// registry (no end-side registrations yet). Always hits SB.
    pub async fn load(&self) -> AppResult<DevicesFile> {
        let path = DEVICES_PATH.replace("{user}", &self.sb_user);
        let body = match self.sb.get_fs(&path).await {
            Ok(b) => b,
            Err(AppError::NotFound(_)) => {
                return Ok(DevicesFile { version: 1, devices: Vec::new() });
            }
            Err(e) => return Err(e),
        };
        serde_json::from_str(&body)
            .map_err(|e| AppError::Internal(format!("devices.json parse: {}", e)))
    }
}
```

Imports needed at top of `src/devices.rs`:
- `std::time::Duration` — already imported (line 6)
- `reqwest` — already a transitive dep, but check `Cargo.toml` allows direct use
- `serde_json` — already imported (line 4)

If `reqwest` is not currently a direct dependency of this crate, add it. Check `Cargo.toml` — if it's only via `proxy.rs` etc., it should still be a workspace dependency. If not present, add `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }` (already in `[dependencies]` line 22 of Cargo.toml — confirmed by reading).

- [ ] **Step 4: 更新 `src/state.rs` 类型引用**

Three edits in `src/state.rs`:

1. Line 2: `use crate::devices::DevicesCache;` → `use crate::devices::{DevicesStore, probe_device_status};` (probe needed later for Task 3)
2. Line 68: `pub devices: Arc<DevicesCache>,` → `pub devices: Arc<DevicesStore>,`
3. Line 106: `let devices = Arc::new(DevicesCache::new(sb.clone(), sb_user.clone(), Duration::from_secs(30)));` → `let devices = Arc::new(DevicesStore::new(sb.clone(), sb_user.clone()));`

- [ ] **Step 5: 运行测试，确认通过**

Run:
```bash
cargo test --lib devices_store_load_hits_sb_every_call probe_device_status_returns_offline_when_unreachable
```
Expected: **PASS**.

- [ ] **Step 6: 编译（再次暴露调用点错误）**

Run:
```bash
cargo build --lib
```
Expected: **WILL FAIL** — `state.users.load_fresh()` calls (in `admin_users_create`) and `state.devices.invalidate()` calls (in seed/delete handlers) no longer exist. Will fix in Task 4.

Do NOT commit yet — Task 4 commits everything together.

---

## Task 3: BFF `me` handler 改写 — 并发 probe + enriched devices

**Files:**
- Modify: `src/routes.rs:291-305`（rewrite the `me` handler）

- [ ] **Step 1: 写失败测试 — `me_returns_enriched_devices_with_runtime_status`**

Append to `tests/bff_integration.rs` (this is the integration suite, not module test):

```rust
// ---------------------------------------------------------------------------
// me returns enriched devices (NEW — PR 2026-09-14 batch API)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn me_returns_enriched_devices_with_runtime_status() {
    let sb = MockServer::start().await;
    let online_dev = MockServer::start().await;
    let offline_dev = MockServer::start().await; // never mounted → refused
    sb_get(&sb, users_fs_path(), 200, &users_json(DEFAULT_DEVICES)).await;
    // online_dev returns /status 200 with a status body
    Mock::given(method("GET"))
        .and(path("/status"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"{"opencode_serve":"running","rathole":"running","system":{"hostname":"box1"},"version":"0.5"}"#,
        ))
        .mount(&online_dev)
        .await;

    let app = test_app(build_state_with_cloud_ip(
        &sb.uri(),
        "127.0.0.1",
        &[("dev-online", online_dev.uri()), ("dev-offline", offline_dev.uri())],
    ));

    let cookie = login(&app, TEST_KEY).await;
    let resp = send(&app, authed_get("/api/me", &cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let devices = body["devices"].as_array().expect("devices[]");
    assert_eq!(devices.len(), 2);

    let online = devices.iter().find(|d| d["pcname"] == "dev-online").unwrap();
    assert_eq!(online["online"], true);
    assert!(online["status"].is_object());
    assert_eq!(online["status"]["opencode_serve"], "running");
    assert_eq!(online["status"]["system"]["hostname"], "box1");

    let offline = devices.iter().find(|d| d["pcname"] == "dev-offline").unwrap();
    assert_eq!(offline["online"], false);
    assert!(offline["reason"].is_string());
}
```

You'll also need these helpers in `tests/common.rs` (or wherever `common::*` lives — `tests/common/mod.rs`):
- `login(&app, key) -> String` — POSTs `/api/login`, returns the cookie value
- `build_state_with_cloud_ip(sb_uri, cloud_ip, port_mappings)` — variant of `build_state` that wires devices with specific URLs

If these don't exist, add them following the existing `build_state` pattern. Search `tests/common/mod.rs` for the existing `build_state` to understand the shape.

- [ ] **Step 2: 运行测试，确认失败**

Run:
```bash
cargo test --test bff_integration me_returns_enriched_devices_with_runtime_status
```
Expected: **FAIL** — `me` handler doesn't include `online` / `status` in `devices[i]` yet (existing assertion `body["devices"][0]["port"] == 4040` still passes; new `online` field missing).

- [ ] **Step 3: 实现新 `me` handler**

Replace `src/routes.rs:291-305` (the entire `me` function body) with:

```rust
async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    let probes = user
        .devices
        .iter()
        .map(|d| probe_device_status(&user.cloud_ip, d.port))
        .collect::<Vec<_>>();
    let results = futures::future::join_all(probes).await;

    let mut enriched: Vec<serde_json::Value> = Vec::with_capacity(user.devices.len());
    for (ud, pr) in user.devices.iter().zip(results.into_iter()) {
        let mut entry = serde_json::json!({
            "desc": ud.desc,
            "name": ud.name,
            "port": ud.port,
            "device-name": ud["device-name"],
            "pctype": ud.pctype,
            "bound": ud.bound,
            "online": pr.online,
        });
        if let Some(r) = pr.reason {
            entry["reason"] = serde_json::Value::String(r);
        }
        if let Some(s) = pr.status {
            entry["status"] = s;
        }
        enriched.push(entry);
    }

    Ok(AxumJson(serde_json::json!({
        "id": user.id,
        "name": user.name,
        "cloud_ip": user.cloud_ip,
        "last_used_at": user.last_used_at,
        "devices": enriched,
        "sb": user.sb,
    })))
}
```

Also add to top-of-file imports (verify):
- `use crate::devices::probe_device_status;` (already added in Task 2 Step 4)
- `use futures::future::join_all;` — add if not present

`futures` is in `[dependencies]` already? Check `Cargo.toml` — `futures` not in deps. Add: `futures = "0.3"` to `[dependencies]` in `Cargo.toml`.

- [ ] **Step 4: 运行测试，确认通过**

Run:
```bash
cargo test --test bff_integration me_returns_enriched_devices_with_runtime_status
```
Expected: **PASS**.

- [ ] **Step 5: 编译整库**

Run:
```bash
cargo build --lib
```
Expected: **WILL FAIL** — `state.users.load_fresh()` (line 1174) and `state.devices.invalidate()` (lines 680, 717) and `state.users.load()` everywhere now use the new types but call signatures changed. Will fix in Task 4.

---

## Task 4: BFF call-site 适配 + `cargo build` 干净

**Files:**
- Modify: `src/routes.rs`（所有 `state.users.load*` / `state.devices.load*` / `state.devices.invalidate` 调用点）
- Modify: `src/users.rs` `#[cfg(test)] mod tests` 中任何引用 `UsersCache` 的地方（如有）

- [ ] **Step 1: 列举所有需要适配的调用点**

Run:
```bash
cd /Users/samuel/Documents/GitForAi/mini-oc-web
grep -n 'state\.users\.\|state\.devices\.' src/routes.rs
```

Expected hits (from earlier reads):
- `state.users.load()` — many sites, all keep working (signature unchanged)
- `state.users.load_fresh()` — line 1174 (`admin_users_create`), needs merge into `load()`
- `state.devices.load()` — many sites, all keep working
- `state.devices.invalidate()` — lines 680, 717 (after seed/delete) — must REMOVE these calls (no cache to invalidate)

- [ ] **Step 2: 适配 `load_fresh` 调用点（合并入 `load`）**

In `src/routes.rs`, the `admin_users_create` function uses `load_fresh` to dedupe against the freshest remote state before minting a 6-digit id. After removing the cache, every `load()` is already fresh. So:

Line 1174: `let mut uf = state.users.load_fresh().await?;` → `let mut uf = state.users.load().await?;`

Verify with grep that no other call site uses `load_fresh`:
```bash
grep -n 'load_fresh' src/
```
Expected: no hits.

- [ ] **Step 3: 删除 `invalidate()` 调用（routes.rs:680, 717）**

Lines 680 and 717 are inside `device_delete` and `devices_seed` respectively. Each calls `state.devices.invalidate().await` after writing to SB. With no cache, these calls are no-ops at best and compile errors at worst.

Two edits:

Line 680:
```rust
state.devices.invalidate().await;
```
→ delete the line.

Line 717: same — delete.

Also remove the `invalidate` impl method from `DevicesStore` (already done in Task 2 Step 3 — we deleted the entire `DevicesCache` block including `invalidate`). Verify with:
```bash
grep -n 'fn invalidate' src/
```
Expected: no hits.

- [ ] **Step 4: 编译**

Run:
```bash
cargo build --lib 2>&1 | tail -20
```
Expected: **SUCCESS** — 0 errors. Warnings about unused imports are OK.

If errors:
- "no method named load_fresh" — Task 4 Step 2 missed a site
- "no method named invalidate" — Task 4 Step 3 missed a site
- "expected UsersStore, found UsersCache" — Task 1/2 struct replacement missed an import

- [ ] **Step 5: 跑全套 Rust 测试**

Run:
```bash
cargo test
```
Expected: **ALL PASS** (or pre-existing failures if any — check before this PR; if pre-existing, document and skip).

- [ ] **Step 6: 提交 Tasks 1-4 的累积改动**

```bash
git add src/users.rs src/devices.rs src/state.rs src/routes.rs tests/bff_integration.rs Cargo.toml
git diff --staged --stat | tee /tmp/task1to4_diffstat.txt
git commit -m "refactor(bff): remove UsersCache/DevicesCache + enrich /api/me with runtime status"
```

Suggested body:
```
- src/users.rs: UsersCache → UsersStore (no TTL, direct SbClient)
- src/devices.rs: DevicesCache → DevicesStore + new probe_device_status helper + DeviceProbeResult type
- src/state.rs: AppState.users/devices hold SbClient-backed stores
- src/routes.rs: me handler rewritten — concurrent probes per device,
  devices[i] now carries online/reason/status fields; load_fresh merged
  into load(); invalidate() calls removed
- tests/bff_integration.rs: me_returns_enriched_devices_with_runtime_status
  covers the new shape; existing me_returns tests updated
- Cargo.toml: add `futures = "0.3"` for join_all
```

---

## Task 5: SPA `UserDevice` schema 扩展（3 个可选运行时字段）

**Files:**
- Modify: `web/src/api.ts:98-110`（`UserDevice` interface）

- [ ] **Step 1: 更新 `UserDevice` interface**

In `web/src/api.ts`, replace the `UserDevice` interface (lines 98-110):

```ts
/** 设备清单条目：描述 + 服务名称 + 端口 + 设备名称（客户端绑定写入）+ 平台类型 + 绑定状态。 */
export interface UserDevice {
  /** 描述（人类可读，展示用；管理表单必填 1-64 字符；旧数据 d-name 已由服务端迁移为 desc）。 */
  desc: string
  /** 设备服务名称（路径/URL 材料）。 */
  name: string
  port: number
  /** 设备名称 —— 仅由绑定客户端（POST /api/device-bind）写入；管理表单只读展示，空 = 尚未绑定。 */
  'device-name': string
  /** 平台类型：'windows' | 'macos'（与 SB 路径列表平台类型同枚举）。 */
  pctype: string
  /** 绑定状态（默认 false）。 */
  bound?: boolean
  // 新增（PR 2026-09-14）：/api/me 返运行时状态；其他接口不返，字段为可选
  /** 是否在线（BFF /api/me 内部探测结果）。 */
  online?: boolean
  /** 探测失败原因（online=false 时填）。 */
  reason?: string
  /** 设备端 /status 响应体（online=true 时填）。 */
  status?: DeviceStatusInfo
}
```

- [ ] **Step 2: 验证类型 + 构建**

Run:
```bash
cd /Users/samuel/Documents/GitForAi/mini-oc-web/web && npm run build 2>&1 | tail -10
```
Expected: **PASS** — `vue-tsc --noEmit` clean, vite build success.

- [ ] **Step 3: 提交**

```bash
git add web/src/api.ts
git commit -m "feat(web): UserDevice schema gains online/reason/status fields"
```

---

## Task 6: SPA `DevicesPage` 简化（删除 `statuses` ref + 单 fetch 刷新）

**Files:**
- Modify: `web/src/pages/DevicesPage.vue`（script + 模板）

- [ ] **Step 1: 删除 `statuses` 相关代码（script setup 上半部）**

Delete these declarations/functions from the `<script setup>` block:

```ts
const statuses = ref<Record<number, DeviceStatus>>({})

function statusOf(d: UserDevice): DeviceStatus | undefined {
  return statuses.value[d.port]
}
```

These are used only for the deleted per-device-status rendering. Keep `DeviceStatus` / `DeviceStatusInfo` type imports (still needed for the optional `status` field on UserDevice), but remove the `DeviceStatus` runtime usage.

- [ ] **Step 2: 重写 helper 函数（基于 `d.status` 读运行时）**

Replace these helpers in the `<script setup>` block:

```ts
/** 卡片主标题：设备名称（客户端绑定写入）→ 描述 → 服务名。 */
function displayName(d: UserDevice): string {
  return d['device-name'] || d.desc || d.name
}

function isOnline(d: UserDevice): boolean {
  return d.online === true
}
function ocServe(d: UserDevice): string {
  return d.status?.opencode_serve ?? 'unknown'
}
function rathole(d: UserDevice): string {
  return d.status?.rathole ?? 'unknown'
}
function hostOf(d: UserDevice): string {
  return d.status?.system?.hostname ?? ''
}

/**
 * 状态点颜色（按优先级）：
 * 红 = 未连接上（探测失败 / rathole 未运行）
 * 黄 = 已连上但不可用（未绑定 / oc serve 未运行）
 * 绿 = 完全可用；灰 = 探测中 / 状态未知（数据未到不闪红黄）
 */
function dotClass(d: UserDevice): string {
  if (d.online === undefined) return 'offline'           // data not arrived yet (silentFirstLoad in flight)
  if (!d.online || rathole(d) !== 'running') return 'danger'
  if (!d.bound || ocServe(d) !== 'running') return 'warn'
  return 'online'
}
```

`isProbing()` is no longer used — the global overlay covers refresh in flight. Delete it.

- [ ] **Step 3: 重写 `refreshAll`（单次 fetch /api/me）**

Replace the entire `refreshAll` function:

```ts
const refreshing = ref<{ done: number; total: number } | null>(null)
let refreshAbort: AbortController | null = null

async function refreshAll() {
  if (refreshing.value !== null) return
  const ctrl = new AbortController()
  refreshAbort = ctrl
  refreshing.value = { done: 0, total: 1 }   // 单次 /api/me 调用

  try {
    try {
      await auth.probe(ctrl.signal)
    } catch (e) {
      if (e instanceof UnauthorizedError) return
      refreshing.value = null
      refreshAbort = null
      showToast('刷新设备清单失败，请稍后重试', 'warn')
      return
    }
    if (ctrl.signal.aborted) return
  } finally {
    const wasCancelled = ctrl.signal.aborted
    refreshing.value = null
    refreshAbort = null
    if (wasCancelled) showToast('已取消刷新', 'muted')
  }
}

function cancelRefresh() {
  refreshAbort?.abort()
}
```

Delete:
- `BATCH = 6` constant
- `refreshAbort` is still needed (keep it)
- All stage (b) logic (the for loop with `Promise.allSettled`, the race guards, the `.catch`)

- [ ] **Step 4: 重写 `silentFirstLoad`（单次 fetch /api.me）**

Replace:

```ts
async function silentFirstLoad() {
  // 1) 拿到最新设备清单（无 signal、不弹菊花）。401 由路由守卫处理。
  await auth.probe().catch(() => {})
}
```

Delete the entire for-loop body (no per-device fetch). Keep `silentFirstLoad` signature unchanged (it's called from `onMounted`).

- [ ] **Step 5: 模板更新 — 删 `v-else-if="isProbing(d)"` + 改卡片状态行**

In the template, find the `<button class="ghost refresh-btn" ...>` line — it should still work, no changes needed.

Find the card status row:

Before:
```html
<div class="card-status muted small">
  <div v-if="statusOf(d)">
    <span :class="ocServe(d) === 'running' ? 'ok-text' : 'warn-text'">
      oc serve {{ ocServe(d) }}
    </span>
    <template v-if="hostOf(d)"> · {{ hostOf(d) }}</template>
  </div>
  <div v-else-if="isProbing(d)">探测中…</div>
  <div v-else>状态未知</div>
</div>
```

After:
```html
<div class="card-status muted small">
  <div v-if="d.online !== undefined">
    <span :class="ocServe(d) === 'running' ? 'ok-text' : 'warn-text'">
      oc serve {{ ocServe(d) }}
    </span>
    <template v-if="hostOf(d)"> · {{ hostOf(d) }}</template>
    <span v-if="!d.online && d.reason" class="warn-text"> · {{ d.reason }}</span>
  </div>
  <div v-else>状态未知</div>
</div>
```

Notes:
- `v-if="statusOf(d)"` → `v-if="d.online !== undefined"` (means "field arrived from server")
- Removed `v-else-if="isProbing(d)"` — refresh is global (overlay), not per-card
- Added reason display when offline (helps debug; was previously hidden)
- `ocServe(d)` etc. now read from `d.status` which is `DeviceStatusInfo | undefined`

- [ ] **Step 6: 构建验证**

Run:
```bash
cd /Users/samuel/Documents/GitForAi/mini-oc-web/web && npm run build 2>&1 | tail -15
```
Expected: **PASS** — `vue-tsc --noEmit` clean. If you see `Cannot find name 'statusOf'` or `isProbing`, a template reference slipped through Step 5 — grep the file for them:
```bash
grep -nE 'statusOf|isProbing|statuses\.' web/src/pages/DevicesPage.vue
```
Expected: no hits.

- [ ] **Step 7: 提交**

```bash
git add web/src/pages/DevicesPage.vue
git commit -m "feat(web): DevicesPage single /api/me call + read runtime from auth.me.devices[i]"
```

---

## Task 7: 范围纪律 + 最终验收

**Files:** (no new file modifications; verification only)

- [ ] **Step 1: 范围扫描 — 未触碰的 BFF 文件**

Verify these files have **no changes** since the previous PR (HEAD~3..HEAD or check `git log --oneline -- <file>`):

```bash
cd /Users/samuel/Documents/GitForAi/mini-oc-web
git log --oneline HEAD~6..HEAD -- src/auth.rs src/error.rs src/jump.rs src/proxy.rs src/sb.rs
```
Expected: no recent commits on these files (they're out of scope).

- [ ] **Step 2: 缓存关键词扫描**

```bash
grep -rnE 'UsersCache|DevicesCache|UsersCache::new|DevicesCache::new' src/
```
Expected: **NO HITS** (the type and impl are fully removed).

```bash
grep -rnE 'load_fresh|invalidate\(\)' src/
```
Expected: **NO HITS** (`load_fresh` merged into `load`; `invalidate` removed).

- [ ] **Step 3: 后端测试套件全绿**

```bash
cargo test
```
Expected: **ALL PASS**. Note: if any tests fail, check whether they're pre-existing failures (run `cargo test` on `HEAD~1` to compare) vs introduced by this PR.

- [ ] **Step 4: SPA 构建**

```bash
cd web && npm run build 2>&1 | tail -5 && cd ..
```
Expected: **PASS** — `vue-tsc --noEmit && vite build` clean.

- [ ] **Step 5: 后端 release 构建**

```bash
cargo build --release
```
Expected: **SUCCESS** — 0 errors, 0 critical warnings.

- [ ] **Step 6: Git 历史清晰**

```bash
git log --oneline -10
```
Expected (commits in order):
- `0585934` docs(spec): /api/me batch return + remove UsersCache/DevicesCache (previous PR)
- `73529e2` docs(plan): align Task 4 §4.2 refreshAll with ADR 19 (previous PR)
- ... (previous PR commits)
- Task 1-4 commit: `refactor(bff): remove UsersCache/DevicesCache + enrich /api/me with runtime status`
- Task 5 commit: `feat(web): UserDevice schema gains online/reason/status fields`
- Task 6 commit: `feat(web): DevicesPage single /api/me call + read runtime from auth.me.devices[i]`

3 commits in this PR (or 4 if Tasks 1-4 were split into 2 — adjust if so).

- [ ] **Step 7: 手动浏览器烟测（可选但推荐）**

If BFF + SPA are running (or you can start them):

1. Login as a user → `/devices` shows 3 cards
2. Each card displays `oc serve` state + hostname from `d.status` (no per-card fetch)
3. Click 刷新状态 → single `/api/me` request in network panel; overlay shows `(0/1)`
4. Cancel mid-flight → toast「已取消刷新」appears, cards unchanged
5. Admin panel: change a user's `bound` → user-side refresh reflects immediately (no 30s wait)

If any check fails, file a follow-up; do not paper over.

---

## Notes for the executor

- **Do NOT add tests** for `/api/device-status/:port` — user explicitly chose "保留路由代码，不写测试" (spec §8.5)
- **Do NOT touch** `src/auth.rs`, `src/error.rs`, `src/jump.rs`, `src/proxy.rs`, `src/sb.rs` — out of scope
- **Do NOT modify** `web/src/components/LoadingOverlay.vue` — works as-is with `total: 1`
- **`UserDevice` schema is a one-way change** — admins (`/api/admin/users`) don't set `online/reason/status`, so the type is forward-compatible (new fields are optional, omitted from admin responses)
- If `cargo build --lib` shows warnings about unused imports in `state.rs` after Task 1/2, leave them or remove if obvious — don't refactor unrelated code

