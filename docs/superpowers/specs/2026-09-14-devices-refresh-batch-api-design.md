# DevicesPage 刷新架构重构：`/api/me` 批量返回运行时状态

**Date:** 2026-09-14
**Status:** Draft (待用户复核)
**Scope:** BFF `src/` 4 文件 + SPA `web/src/` 3 文件
**Out of Scope:** admin_* 路由、proxy.rs（项目/会话代理）、前端 ProjectsPage/SessionsPage、admin 后台

> 把 DevicesPage 的「刷新设备状态」从「1× `/api/me` + N× `/api/device-status/:port`」重构为「1× `/api/me`（已含运行时）」。同步去掉 `UsersCache` 和 `DevicesCache` 的 30 秒 TTL（架构不需要）。

---

## 1. 背景与现状

### 1.1 当前架构（2026-09-14 DevicesPage 刷新特性后）

用户点「刷新状态」：

```
SPA refreshAll():
  (a) auth.probe(ctrl.signal)  →  GET /api/me                  (1×)
  (b) 并发 fetch N× /api/device-status/{port}                  (N×)
       BFF 内部 → GET http://{cloud_ip}:{port}/status          (N×)
```

BFF 端：

- `state.users.load()`：`UsersCache` 30 秒 TTL 包装 `SbClient::get_fs("web/opencode/config.md")`
- `state.devices.load()`：`DevicesCache` 30 秒 TTL 包装 `SbClient::get_fs("serv/opencode/{user}/devices.json")`
- `device_status` handler（`routes.rs:524`）：校验 port 属于用户 → 用空凭据 `DeviceClient` 探 `{cloud_ip}:{port}/status` → 4s 超时

SPA 端：

- `auth.me.devices[]` 只有 metadata（id/name/port/bound/device-name/pctype）
- `statuses: Record<number, DeviceStatus>` 单独 ref，存每台设备的运行时
- DevicesPage.vue:130 在 refreshAll stage (b) 写入；:171 在 silentFirstLoad 写入

### 1.2 问题分析

- **2 次往返**：先 /api/me 拿元数据，再 N 次 /api/device-status 拿运行时。N 越大，菊花遮罩时长越长
- **状态分散**：`auth.me.devices[i]`（元数据）和 `statuses[port]`（运行时）两个独立来源，`dotClass` / `ocServe` 等 helper 都要从两边拼装
- **缓存导致 stale**：`UsersCache` 30s 内 admin 改设备授权不生效；`DevicesCache` 30s 内端侧注册不生效
- **架构无意义的小用户量场景**：单租户、低频点击，SB 直打成本可接受

---

## 2. 目标架构

### 2.1 单次往返

```
SPA refreshAll():
  auth.probe(ctrl.signal)  →  GET /api/me  (含 user.devices[i].{online, status})
       ↑ 1× 调用，菊花遮罩覆盖整个请求
```

BFF `/api/me` 内部：

```
load users (直打 SB，无缓存)  →  for each user.devices[i]: probe {cloud_ip}:{port}/status
                              →  拼装 enriched devices[] 返回
```

### 2.2 缓存策略

- `UsersCache` **删除**：`state.users` 改为直接持有 `Arc<SbClient>`，每次 `/api/me` / admin 操作都直打 SB
- `DevicesCache` **删除**：同上；`/api/devices` 路由每次直打 SB（端侧注册变更可秒级反映）
- `/api/device-status/:port` **保留**（路由代码不动），作为未来「单台详情深查」接口

### 2.3 SPA 状态

- `auth.me.devices[i]` 直接含运行时字段（`online`, `reason?`, `status?`）
- 删除 `statuses` ref
- DevicesPage.vue 的 `statusOf(d)` / `ocServe(d)` / `rathole(d)` / `hostOf(d)` 改为读 `d.status`

---

## 3. 数据契约变化

### 3.1 `MeResponse` schema（前端 `web/src/api.ts:120`）

**变更前**：
```ts
export interface MeResponse {
  id: string
  name: string
  cloud_ip: string
  last_used_at?: string
  devices: UserDevice[]       // 仅元数据
  sb: SbConfig
}
```

**变更后**：
```ts
export interface MeResponse {
  id: string
  name: string
  cloud_ip: string
  last_used_at?: string
  devices: UserDevice[]       // 元数据 + 运行时（见下）
  sb: SbConfig
}

export interface UserDevice {
  desc: string
  name: string
  port: number
  'device-name': string
  pctype: string
  bound?: boolean
  // 新增（/api/me 返；/api/admin/users 等其他接口不返，字段为可选）
  online?: boolean
  reason?: string
  status?: DeviceStatusInfo  // 复用既有 DeviceStatusInfo 类型
}
```

向后兼容：`online`/`reason`/`status` 都是可选，旧调用方（admin / user info）不受影响。

### 3.2 BFF `/api/me` Rust 返回结构

变更前（`routes.rs:291-303`）：
```json
{
  "id": "...", "name": "...", "cloud_ip": "...",
  "last_used_at": "...",
  "devices": [{ "desc":"...", "name":"...", "port":N, "device-name":"...", "pctype":"...", "bound":true }],
  "sb": { ... }
}
```

变更后：
```json
{
  "id": "...", "name": "...", "cloud_ip": "...",
  "last_used_at": "...",
  "devices": [
    {
      "desc":"...", "name":"...", "port":N, "device-name":"...", "pctype":"...", "bound":true,
      "online": true,
      "status": { "opencode_serve":"running", "rathole":"running", "system":{...}, "version":"..." }
    },
    {
      "desc":"...", "name":"...", "port":M, "device-name":"...", "pctype":"...", "bound":false,
      "online": false,
      "reason": "device returned HTTP 502"
    }
  ],
  "sb": { ... }
}
```

### 3.3 BFF probe 内部逻辑

新增 helper（建议放在 `src/proxy.rs` 或 `src/devices.rs`）：

```rust
/// 用空凭据探测 {cloud_ip}:{port}/status，4s 超时。
/// 永不抛错——失败返 online=false + reason。
async fn probe_device_status(cloud_ip: &str, port: u16) -> DeviceProbeResult {
    let url = format!("http://{cloud_ip}:{port}/status");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build();
    match http {
        Err(e) => DeviceProbeResult { online: false, reason: format!("client: {e}") },
        Ok(c) => match c.get(&url).send().await {
            Ok(r) if r.status().is_success() => match r.json::<serde_json::Value>().await {
                Ok(s) => DeviceProbeResult { online: true, status: Some(s) },
                Err(e) => DeviceProbeResult { online: false, reason: format!("parse: {e}") }
            },
            Ok(r) => DeviceProbeResult { online: false, reason: format!("HTTP {}", r.status()) },
            Err(e) => DeviceProbeResult { online: false, reason: e.to_string() }
        }
    }
}
```

`me` handler 内部：

```rust
async fn me(...) -> AppResult<AxumJson<serde_json::Value>> {
    let user = current_user(&state, &headers).await?;
    // 注：current_user 改为直接 sb.get_fs，无缓存
    let mut enriched = Vec::with_capacity(user.devices.len());
    // 并发探测：futures::future::join_all
    let probes = user.devices.iter()
        .map(|d| probe_device_status(&user.cloud_ip, d.port))
        .collect::<Vec<_>>();
    let results = futures::future::join_all(probes).await;
    for (ud, pr) in user.devices.iter().zip(results) {
        enriched.push(json!({
            "desc": ud.desc,
            "name": ud.name,
            "port": ud.port,
            "device-name": ud.device-name,
            "pctype": ud.pctype,
            "bound": ud.bound,
            "online": pr.online,
            "reason": pr.reason,
            "status": pr.status,
        }));
    }
    Ok(AxumJson(json!({
        "id": user.id,
        "name": user.name,
        "cloud_ip": user.cloud_ip,
        "last_used_at": user.last_used_at,
        "devices": enriched,
        "sb": user.sb,
    })))
}
```

并发探测：N ≤ 10 时单批 join_all；N > 10 时分批 6（沿用 ADR #14 的 Chrome 同源 6 连接上限）。每个探测 4s 超时。

### 3.4 `UsersCache` 删除

`src/users.rs` 当前结构（简化）：
```rust
pub struct UsersCache { sb: Arc<SbClient>, sb_user: String, ttl: Duration, state: Mutex<...> }
impl UsersCache {
    pub async fn load(&self) -> AppResult<UsersFile> { ... }       // 30s TTL 包装
    pub async fn load_fresh(&self) -> AppResult<UsersFile> { ... } // 跳过缓存
    pub async fn save(&self, uf: &UsersFile) -> AppResult<()> { ... } // 写 SB
}
```

改为：
```rust
// UsersCache 删除；UsersFile 直接走 SbClient

pub struct UsersStore { sb: Arc<SbClient>, sb_user: String }
impl UsersStore {
    pub async fn load(&self) -> AppResult<UsersFile> { /* 直打 SB */ }
    pub async fn save(&self, uf: &UsersFile) -> AppResult<()> { /* 写 SB */ }
}
```

调用点适配：
- `state.users.load()` → `state.users.load()`（同名，签名不变；不再有缓存层）
- `state.users.load_fresh()` → 合并入 `load()`（每次都是 fresh）
- `state.users.save(...)` → 不变

### 3.5 `DevicesCache` 删除

同理：删除 `DevicesCache` 30s TTL 包装，`/api/devices` 每次直打 SB。admin 的 seed/delete 调用前不再需要 `invalidate()`（因为没有缓存可清）。

---

## 4. SPA 改动

### 4.1 `web/src/pages/DevicesPage.vue` 大幅简化

**删除**：
- `statuses: ref<Record<number, DeviceStatus>>({})`
- `probing` ref（已删过）
- `isProbing()` 函数（已删过）
- `refreshAbort: AbortController | null` 仍保留（cancel 仍需要）
- `BATCH = 6` 常量

**改写 `refreshAll`**：
```ts
async function refreshAll() {
  if (refreshing.value !== null) return
  const ctrl = new AbortController()
  refreshAbort = ctrl
  refreshing.value = { done: 0, total: 1 }   // 1× fetch

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
```

**改写 `silentFirstLoad`**：
```ts
async function silentFirstLoad() {
  await auth.probe().catch(() => {})
}
```

**改写 helper 函数**：
```ts
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
```

`dotClass` 改 `if (!d.online || rathole(d) !== 'running') return 'danger'` 等。

**LoadingOverlay 调用简化**：进度 `(n/N)` 改为 `(0/1)` 或固定标题。

### 4.2 LoadingOverlay 进度文案

`progress?: { done: number; total: number }` 改成支持 `total = 1` 即可显示 `(0/1)`。或者直接隐藏进度，只显示标题。**本次选前者**（保持 LoadingOverlay 不变，progress 自然显示 `(0/1)`）。

### 4.3 `web/src/store.ts`

`useAuthStore.probe()` 已支持 signal。无需改动。

### 4.4 `web/src/api.ts`

- `UserDevice` 接口增加 3 个可选字段（`online?`, `reason?`, `status?`）
- `DeviceStatus` / `DeviceStatusInfo` 类型保留（admin / 单台接口仍可能用）

---

## 5. 错误处理矩阵

| 场景                                | `/api/me` 返                                                                                | SPA 行为                                |
| ----------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------- |
| `/api/me` 401                       | 401 + `{error.code: "unauthorized"}`                                                         | refreshAll → return；router 守卫跳 /login |
| `/api/me` 5xx（SB 不可达）          | 503 + `{error.code: "service_unavailable"}`                                                  | `me===null` → refreshAll 显式 push('/login')；`me!==null` → warn toast |
| `/api/me` 200，单台 probe 失败       | 200 + `devices[i].online=false, reason="..."`                                                | 卡片渲染为 danger（红点）+ reason       |
| `/api/me` 200，全部 probe 失败       | 200 + 所有 `devices[i].online=false`                                                          | 所有卡片 danger                          |
| `/api/me` 200，单台 probe 成功        | 200 + `devices[i].online=true, status={...}`                                                 | 卡片根据 status 渲染                     |
| 用户取消                             | 客户端 abort → fetch reject AbortError                                                       | finally 走「已取消刷新」toast           |

注：表格中「`me===null` 判别」沿用 ADR 19（`auth.probe()` 的 `me===null` ⇔ 401 约定）。

---

## 6. 取消语义

- 客户端：`AbortController` 传给 `auth.probe(ctrl.signal)`
- BFF `/api/me` handler **不接收 AbortSignal**：内部 N 个 probe 仍然跑完（4s × N 上限保证总时长可控）
- 客户端 abort 后：fetch reject AbortError → refreshAll finally → toast「已取消刷新」
- 复杂度权衡：BFF 透传 abort 需要 plumb 到 reqwest cancellation token，**收益小**（单租户低频），**不做**

---

## 7. 文件改动清单

| 文件                                       | 改动                                                                                                       | 文件数       |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------- | ------------ |
| `src/users.rs`                              | 删除 `UsersCache`，新增 `UsersStore`（直接 SbClient）                                                       | 1            |
| `src/devices.rs`                            | 删除 `DevicesCache`，新增 `DevicesStore`；新增 `probe_device_status` helper + `DeviceProbeResult` 类型        | 1            |
| `src/state.rs`                              | `users: Arc<UsersCache>` → `users: Arc<UsersStore>`（同名改名）；`devices` 同理                              | 1            |
| `src/routes.rs` `me` handler                | 改写：load users → 并发 probe → enrich devices → 返回                                                       | 1            |
| `src/routes.rs` 其他 handler 调用点          | `state.users.load()` / `state.devices.load()` → 适配新类型签名（同名，签名不变；只是无缓存）                | 1            |
| `web/src/api.ts`                            | `UserDevice` 加 3 个可选字段                                                                                | 1            |
| `web/src/store.ts`                          | 无改                                                                                                        | 0            |
| `web/src/pages/DevicesPage.vue`            | 大幅简化：`refreshAll` / `silentFirstLoad` / helper / `dotClass` / 删除 `statuses` ref                       | 1            |
| `web/src/components/LoadingOverlay.vue`    | 无改（progress 总数 1 自然显示 `(0/1)`）                                                                     | 0            |
| `src/routes.rs` `/api/device-status/:port` | 不改（保留路由）                                                                                              | 0            |
| `tests/bff_integration.rs` + `tests/*`     | 更新 `me_returns_*` 断言新结构                                                                              | 2+          |
| `web/src/pages/ProjectsPage.vue` 等        | 无改（继续读 `auth.me.devices` 元数据，不读运行时字段）                                                      | 0            |

总计：**9 个文件改动**（BFF 5 + SPA 2 + 测试 2+），**3 个文件不动**。

---

## 8. 验收标准

### 8.1 功能

- [ ] 用户点「刷新状态」→ 单次 `/api/me` 调用 → 菊花遮罩出现 → 完成后所有卡片显示最新运行时状态
- [ ] `/api/me` 响应中 `devices[i]` 含 `online` / `status` 字段（成功）/ `online=false, reason` 字段（失败）
- [ ] 用户点「取消」→ 菊花关闭 + toast「已取消刷新」
- [ ] 菊花遮罩进度显示 `(0/1)`（或被 LoadingOverlay 优雅处理）
- [ ] dotClass 行为不变：online+oc_serve+rathole+bound 全部正确时显示绿点；否则按优先级显示黄/红/灰
- [ ] `/api/device-status/:port` 路由仍然可调（curl 直接打返 200/502，行为与本 PR 前一致）

### 8.2 缓存去除

- [ ] `UsersCache` 关键词在 `src/` 完全不出现
- [ ] `DevicesCache` 关键词在 `src/` 完全不出现
- [ ] `state.users` / `state.devices` 现在是 `Arc<SbClient>`（或新 `UsersStore` / `DevicesStore`），无 TTL 字段
- [ ] admin `/api/admin/users` 创建用户后，普通用户立即（不需等 30s）能看到新设备名出现在 `auth.me.devices`
- [ ] 端侧 PUT `devices.json` 后，普通用户立即能看到 `public_url` / `online` 反映

### 8.3 兼容

- [ ] `admin_users_list` / `admin_info` 等路由不受影响（不读 `online` / `status` 字段）
- [ ] ProjectsPage / SessionsPage / UserSettingsDialog 不受影响（继续用 `PortalUser.devices` 元数据）
- [ ] `DeviceStatus` / `DeviceStatusInfo` 类型保留（`/api/device-status` 仍可能独立使用）

### 8.4 构建 / 测试

- [ ] `cd web && npm run build` exit 0
- [ ] `cargo test --test bff_integration` 全绿
- [ ] `cargo build --release` exit 0

### 8.5 文档

- [ ] `docs/superpowers/specs/2026-09-14-devices-refresh-loading-design.md`（前 PR 的 spec）**保留**——它描述的是 AbortSignal + LoadingOverlay 模式，本 PR 的架构改动是它的**演化版本**，不冲突
- [ ] 本 spec（`docs/superpowers/specs/2026-09-14-devices-refresh-batch-api-design.md`）Approved 后，作为本 PR 的权威设计

---

## 9. ADR（本设计增量）

| #  | 决策                                                          | 理由                                                                                                                          |
| -- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| 19 | `/api/me` 改为批量返回 devices + 运行时                       | 单次往返替代 N+1；架构简化（无 statuses 双源）；用户量小场景下 N 个并发 probe 的延迟可控                                       |
| 20 | 删除 `UsersCache` 30s TTL                                     | 用户量小、点击不频繁、SB 可承受；缓存带来的 stale 风险大于性能收益                                                            |
| 21 | 删除 `DevicesCache` 30s TTL（同 20）                          | 同上；admin seed/delete 的 invalidate() 不再需要                                                                                |
| 22 | `/api/device-status/:port` 保留路由（不写测试）                | 未来「单台详情深查」接口；YAGNI 不删，但本次不加测试覆盖                                                                       |
| 23 | `/api/me` handler 不接收 AbortSignal                           | 实现复杂（BFF 内部 N probe 需透传 reqwest cancellation token）；客户端 abort 即关连接，BFF 工作量浪费但不阻塞；N ≤ 10 时 4s × N 上限可控 |

---

## 10. 不做（明确延后）

| 项目                                              | 延后原因                                                                       |
| ------------------------------------------------- | ------------------------------------------------------------------------------ |
| `/api/device-status` 集成测试                     | 按用户指示「保留路由代码，不写测试」                                            |
| BFF `/api/me` 内部 probe 也透传 AbortSignal       | 实现复杂度高，收益小（见 ADR 23）                                                |
| ProjectsPage / SessionsPage 改用同一 enriched devices 模式 | 本次只重构刷新流程；其他页面的运行时数据需求不在 scope 内                     |
| 把 `/api/me` 改成 GraphQL / 分页                  | YAGNI：当前用户量小，3 设备以内                                                                           |
| 客户端 stale-while-revalidate                       | 缓存架构选择：直接每次打，不引入 stale-while-revalidate 复杂度                   |
