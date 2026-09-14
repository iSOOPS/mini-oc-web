# mini-oc-web 实施缺口分析与补全计划

**Date:** 2026-09-12
**Status:** Approved (用户确认)
**Scope:** 本次 PR 范围内可立即补全的 BFF + SPA 缺陷修复
**Out of Scope:** Phase 2/3 功能、mini-oc-gui 端侧改造、协议字段不匹配的端侧联动修复

> 本文档是 `2026-08-28-mini-oc-web-impl-design.md` 的**实施现状快照**，记录在原始设计文档 Approved 之后到本次复盘之间发现的 12 项缺口、其优先级判定、本次 PR 范围、以及对每个 Phase 2/3 项目的明示延后。

---

## 1. 调研结论

本次复盘对三个相关代码库做了完整通读：

| 调研对象                                              | 调研目的                          | 关键发现                                                     |
| ----------------------------------------------------- | --------------------------------- | ------------------------------------------------------------ |
| `D:\GitForAi\mini-oc-web` (本仓库)                    | 识别未实现的功能                   | 8 项可立即补全（4 项 Rust / 4 项 SPA）                         |
| `D:\GitForAi\mini-oc-gui` (端侧)                       | 核对 BFF ↔ 端侧协议契约一致性       | 4 处字段不匹配 + 端侧无 devices.json 上报逻辑                  |
| `D:\服务器配置\8.159.159.138` (生产部署目录)            | 核对部署架构与 BFF 上线要求         | cookie_key volume 缺失 / 多个 CHANGE_ME 占位符 / 安全审计通过 |

---

## 2. 缺口清单与本次 PR 范围

### 2.1 本次补全（8 项）✅

| #  | 缺口                                          | 文件                                | 优先级 | 复杂度   |
| -- | --------------------------------------------- | ----------------------------------- | ------ | -------- |
| 1  | `manual_create_session` 端点返回硬编码错误    | `src/routes.rs:393-403`             | 高     | 小       |
| 2  | `/api/devices` 用占位符凭据 `"x", "x"` 探活   | `src/routes.rs:218-220`             | 中     | 极小     |
| 3  | `bff_integration.rs` 是 `assert!(true)` 占位符 | `tests/bff_integration.rs`          | 高     | 中       |
| 4  | `SessionsPage` 前端自己计算 jump URL           | `web/src/pages/SessionsPage.vue:61` | 中     | 小       |
| 5  | `AuthStore` 刷新页面丢失登录态                | `web/src/store.ts`                  | 高     | 小       |
| 6  | `DevicesPage` `labelBadge` 返回空字符串       | `web/src/pages/DevicesPage.vue:51`  | 低     | 极小     |
| 7  | `SettingsPage` 无 `manual_devices` 管理 UI     | `web/src/pages/SettingsPage.vue`    | 中     | 中       |
| 8  | `onSeed` 跳 settings 占位，无真 seed           | `web/src/pages/DevicesPage.vue:65`  | 中     | 小       |

### 2.2 本次明确延后（4 项）⏸️

| #  | 缺口                          | 延后原因                                                          | 落地位置                          |
| -- | ----------------------------- | ----------------------------------------------------------------- | --------------------------------- |
| 9  | iframe 预热跳转               | 设计文档 §17 明确归 Phase 2，需要新增 `JumpPreviewPage.vue`       | 后续 Phase 2 PR                   |
| 10 | path-list sections 元数据升级 | 端侧改动未就绪，强推进会导致 BFF/端侧协议不一致                   | 等端侧 + Phase 2 PR               |
| 11 | nginx `auth_request` 全域 SSO | 设计文档 §17 明确归 Phase 3，依赖 §10.4 安全审计                  | 后续 Phase 3 PR                   |
| 12 | mini-oc-gui 端侧 devices.json 上报 | 本次 PR 仅在 BFF 加 seed 接口兜底，端侧改造由独立 PR 负责          | 端侧仓库独立 PR                   |

---

## 3. 设计文档补全细节

### 3.1 `manual_create_session` 端点补全

**当前实现**（src/routes.rs:393-403）：
```rust
async fn manual_create_session(...) -> Result<...> {
    require_session(...)?;
    validate_pcname(&device_id)?;
    let _ = (state, device_id);
    Err(AppError::Internal("manual devices not yet wired".into()))
}
```

**补全设计**：
- 输入：`POST /api/manual/{device_id}/sessions` + `Json<CreateSessionBody>{directory, title}`
- 步骤：
  1. 校验 `device_id` 走 pcname 白名单（已实现）
  2. 读取 config.md（当前端登录用户的 pcname → query pcname 或读 localStorage）—— **简化**：先固定从 query 拿 `?pcname=` 参数
  3. 在 `cfg.manual_devices[]` 中查找 `id == device_id` 的条目
  4. 用其 `base_url` 构造临时 `DeviceClient`
  5. 调用 `client.create_session(directory, title)` 拿 `CreatedSession`
  6. 调用 `compute_jump_url` 时把 `manual_device.base_url` 作为 `override_url` 注入（优先级最高）
  7. 返回 `{jump_url, id, directory}`

**接口契约新增**：
```rust
// routes.rs: 新增 query 参数
struct ManualSessionsQuery {
    pcname: String,   // 当前登录用户的 pcname，从 URL query 拿
}
```

### 3.2 BFF `seed` 端点（补端侧缺口）

为不阻塞前端本地调试，新增 `POST /api/devices/seed`：
- 接收 `{pcname, public_url, pctype}` 三字段
- 校验 pcname 白名单
- 写一条 demo device 到 SB 的 `serv/opencode/{user}/devices.json`（不覆盖已有列表）
- 返回 `{ok: true}`
- **标注**：端侧正式上线后此端点保留作为本地辅助，但不推荐生产使用

### 3.3 `/api/devices` 健康探活凭据修复

当前 `DeviceClient::new(&d.public_url, "x", "x")` 凑巧能工作（health 不验账密），但语义不对。改为：
```rust
let client = DeviceClient::new(&d.public_url, "", "");
```
明确表示这是免鉴权调用，零凭据。

### 3.4 SPA SessionsPage 改调 `/jump`

**当前**：前端 `jumpHref` 用 `btoa(encodeURIComponent(directory))` 自己算
**改后**：在 `onMounted` 拉 `GET /api/devices/:pctype/:pcname/jump?directory=...&session={sid}&format=json`，把返回的 `jump_url` 渲染到 `<a href>`
**降级**：如果 `/jump` 调用失败，回退到前端本地编码（保留旧逻辑作为兜底）

### 3.5 SPA Store 登录态探测

**当前**：`store.ts` 初始化 `username = null`，只有调用过 login 才会同步
**改后**：在 `setup()` 时调用一次 `apiGet('/api/devices')`：
- 200 → 登录态有效，更新 `username`
- 401 → 未登录，清空 store（不报错）
- 其他错误 → 不处理

### 3.6 SPA SettingsPage manual_devices UI

**新增 section**：与现有 `jump_overrides` 同风格
- 列表展示每条 `manual_device {id, label, base_url}`
- "新增" 按钮弹出表单（id / label / base_url 三字段）
- 每条右侧有"删除"按钮
- 与 jump_overrides 一样使用同一个 PUT `/api/config` 提交

### 3.7 SPA DevicesPage labelBadge

把硬编码空字符串改为：
```ts
const labelBadge = computed(() => '本地覆盖')  // 或英文 "overridden"
```

### 3.8 bff_integration 真实集成测试

**新增测试用例**（用 `axum-test` + `wiremock` 启完整 router + mock SB/设备）：
1. `healthz_returns_ok`
2. `login_success_sets_cookie`
3. `login_wrong_password_returns_401`
4. `login_rate_limited_after_5_failures`
5. `devices_requires_session`
6. `devices_returns_empty_when_no_devices`
7. `devices_returns_online_status_from_health`
8. `config_get_returns_default_when_missing`
9. `config_put_then_get_round_trips`
10. `device_projects_returns_proxy_data`
11. `device_sessions_returns_proxy_data`
12. `device_create_session_returns_jump_url`
13. `manual_create_session_returns_jump_url`
14. `seed_writes_demo_device`

---

## 4. 协议不一致项（本次不动）

调研发现 BFF ↔ 端侧 mini-oc-gui 在以下位置存在字段不匹配。本次 PR **不修复**，但在文档中明示：

| 位置                                | BFF 期望                                                  | 端侧实际                                                       | 影响             |
| ----------------------------------- | --------------------------------------------------------- | -------------------------------------------------------------- | ---------------- |
| `GET /project` 响应                  | `Vec<{path, lastOpenedAt}>`                                | `Vec<{id, path, name, sessions: Vec<Session>}>`                | 丢失 sessions 等 |
| `GET /session?directory=` 响应       | `Vec<{id, title, updatedAt}>`                              | `Vec<{id, title, directory, created_at, updated_at}>`           | 丢失 directory   |
| `POST /api/session` 请求             | `{directory, title}`                                       | `{title, location: {directory}}`                                | **请求格式错**    |
| `POST /api/session` 响应             | `{id, directory}`                                          | `{data: {id, title, directory}}`                                | 响应结构错       |
| 端侧 `devices.json` 上报             | （读 SB）                                                  | **未实现**                                                     | 列表永远为空     |

**未来 PR**：建议在 mini-oc-gui 仓库建 issue 跟踪，本次不阻塞 BFF 上线。

---

## 5. 部署架构审计

### 5.1 与 BFF 相关的配置现状

| 现状                     | 评价     | 备注                                                         |
| ------------------------ | -------- | ------------------------------------------------------------ |
| nginx upstream 已配置     | ✅       | `oc.isoops.com → mini_oc_web:8100`                           |
| `/healthz` 探测           | ✅       | docker-compose healthcheck 已设                               |
| WebSocket 透传            | ✅       | `oc.isoops.com` server block 已有 upgrade headers            |
| rathole 三隧道            | ✅       | yg-win / office-mac / home-win，已映射到内网 4040/4041        |
| SilverBullet              | ✅       | `silverbullet:3000`，nginx upstream `md.isoops.com`         |
| `mini-oc-web/data` volume | ⚠️       | docker-compose 缺宿主机路径映射，会导致 cookie_key 丢失        |
| `CHANGE_ME_BEFORE_DEPLOY` 占位符 | ❌ | 用户**部署前必须替换**为真密码                                  |
| nginx `auth_request` SSO | ❌        | Phase 3，不在本次范围                                          |

### 5.2 上线检查清单（用户自行执行）

1. [ ] 替换 docker-compose.yml 中的两个 `CHANGE_ME_BEFORE_DEPLOY`
2. [ ] 在 docker-compose.yml `mini-oc-web.volumes` 补上 `$PWD/mini-oc-web/data:/data`
3. [ ] 确认 `$PWD/mini-oc-web/data` 目录存在
4. [ ] `docker compose config` 校验语法
5. [ ] `docker compose up -d mini-oc-web` 启动
6. [ ] `curl -I https://oc.isoops.com/healthz` 期望 200

---

## 6. 本次 PR 验收标准

| 验收项                                          | 验证方式                                | 通过判据      |
| ----------------------------------------------- | --------------------------------------- | ------------- |
| Rust 后端 8 项补全                              | `cargo build --release`                  | 无 error/warning |
| 全部单测                                        | `cargo test`                            | 全绿           |
| 集成测试 14 个用例                               | `cargo test --test bff_integration`     | 全绿           |
| SPA 改动                                       | `cd web && npm install && npm run build` | 构建成功        |
| TypeScript 类型检查                              | `npm run type-check` (如有)              | 无 error       |
| 设计文档                                       | 本文件                                  | 已 commit      |

---

## 7. ADR（本次增量）

| #  | 决策                                                                | 理由                                                                |
| - | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| 9 | `manual_create_session` 用 query `?pcname=` 标识当前登录用户         | 避免要求所有调用方额外保存 pcname 到 store，URL 路径即足够             |
| 10 | `seed` 端点用 POST 而不是 GET                                       | GET 不应修改服务端状态（devices.json 是写操作）                       |
| 11 | `/jump` 调用失败时 SPA 降级到本地 base64                              | 与端侧 `/api/session` 调用模式一致，不阻塞前端跳转                     |
| 12 | 集成测试用 `axum-test` + `wiremock` 而不是 `mockito`                   | BFF 已用 wiremock + tokio，避免引入新依赖                              |
| 13 | `/api/devices` health 探活不传任何凭据                                | 显式表达"无需鉴权"语义，避免未来误传真账密到日志                       |

---

## 8. 后续 PR 追踪

| 编号 | 内容                                          | 触发条件                       |
| ---- | --------------------------------------------- | ------------------------------ |
| F-1  | mini-oc-gui 端侧 devices.json 自动上报        | 等端侧 release 窗口             |
| F-2  | BFF ↔ 端侧 协议字段统一                       | F-1 完成后                     |
| F-3  | Phase 2 iframe 预热跳转                       | 用户启用 Phase 2                |
| F-4  | Phase 2 path-list sections 元数据             | F-1 + F-2 后                    |
| F-5  | Phase 3 nginx auth_request SSO                | 用户启用 Phase 3                |
