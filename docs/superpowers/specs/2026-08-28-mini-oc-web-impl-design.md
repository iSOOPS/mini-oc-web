# mini-oc-web 实施设计（Phase 1 MVP）

**Date:** 2026-08-28
**Status:** Approved
**Source design:** [`2026-08-28-mini-co-web-design.md`](../../2026-08-28-mini-co-web-design.md)（v2，668 行）
**Scope:** 本仓库（mini-oc-web 门户 BFF + SPA 占位）+ 服务器部署文件增量

> 命名说明：本仓库将设计文档中的 `mini-co-web` 统一替换为 `mini-oc-web`（与本仓库目录名一致）。设计文档作为参考仍可读 `mini-co-web` 字样；本仓库内一律使用 `mini-oc-web`。

---

## 1. 目标与范围

### 1.1 In Scope（做）

- 本仓库实现 mini-oc-web BFF + SPA 占位 + Dockerfile（多阶段构建）
- 服务器部署文件增量：`/Users/samuel/Documents/Server/8.159.159.138/` 下 `docker-compose.yml` 与 `nginx/nginx.conf` 的 mini-oc-web 服务块 / server 块
- Phase 1（MVP）端到端契约：登录 → 设备列表 → 项目列表 → 会话列表 → 跳转 oc web
- TDD 节奏：纯逻辑模块（jump / base_url 校验 / pcname 白名单）先写单测再写实现

### 1.2 Out of Scope（不做）

- mini-oc-gui 端侧改造（设计文档 §8 全 4 项）—— 用户另行安排
- Phase 2（iframe 预热 / path-list sections 元数据升级）
- Phase 3（nginx `auth_request` 全域 SSO）
- 完整 Vue SPA 业务逻辑（占位页 + 路由 + API 客户端骨架即可）
- Redis 接入（设计文档明确暂不使用）
- Docker 镜像推送 harbor（用户自行操作）

---

## 2. 仓库结构

```
mini-oc-web/                                # /Users/samuel/Documents/GitForAi/mini-oc-web/
├── Cargo.toml                              # bin: mini-oc-web
├── Dockerfile                              # 多阶段（rust + node），预留 web/dist
├── docker-compose.yml                      # 本地自包含运行
├── .env.example                            # WEB_PORT / 统一账密 / SB 配置
├── README.md                               # 简明启动 + 部署指引
├── src/
│   ├── main.rs                             # 启动 + 路由挂载 + 健康检查
│   ├── auth.rs                             # login/logout + HMAC Cookie + IP 限速
│   ├── sb.rs                               # SilverBullet HTTP 客户端
│   ├── devices.rs                          # devices.json 读取 + 30s TTL 内存缓存
│   ├── config_md.rs                        # serv/web/{pcname}/config.md 读写
│   ├── proxy.rs                            # 设备 API 出站（health/project/session/创建）
│   ├── jump.rs                             # 跳转 URL 生成（base64url，单测锁定）
│   └── routes.rs                           # /api/* 路由 + 静态资源服务 + 错误统一化
├── web/                                    # SPA（Vite + Vue 3）
│   ├── package.json                        # 占位骨架
│   ├── vite.config.ts
│   ├── tsconfig.json
│   ├── index.html
│   ├── src/
│   │   ├── main.ts
│   │   ├── App.vue
│   │   ├── router.ts                       # /login /devices /projects /sessions /settings
│   │   ├── api.ts                          # fetch 封装（带 cookie）
│   │   └── pages/
│   │       ├── LoginPage.vue               # 占位
│   │       ├── DevicesPage.vue             # 占位
│   │       ├── ProjectsPage.vue            # 占位
│   │       ├── SessionsPage.vue            # 占位
│   │       └── SettingsPage.vue            # 占位
│   ├── dist/                               # 空目录占位（README 说明需 npm run build）
│   └── README.md                           # SPA 构建说明
├── tests/
│   └── bff_integration.rs                  # 集成测试（mock 设备响应）
└── docs/
    └── superpowers/
        └── specs/
            └── 2026-08-28-mini-oc-web-impl-design.md  # 本文件
```

---

## 3. 模块边界与依赖

```
main.rs ─┬─ auth.rs（Cookie HMAC + 限速）
         ├─ sb.rs（SB HTTP 客户端，无业务依赖）
         ├─ devices.rs ─── sb.rs
         ├─ config_md.rs ── sb.rs
         ├─ proxy.rs（设备 API 出站，无业务依赖）
         ├─ jump.rs（纯函数 + 单测，零依赖）
         └─ routes.rs ─┬─ auth.rs
                       ├─ devices.rs
                       ├─ config_md.rs
                       ├─ proxy.rs
                       └─ jump.rs
```

**约束**：
- 每个模块独立可测，依赖单向，无循环
- `jump.rs` 零依赖（纯函数 + `base64` + `urlencoding` crate），便于单测锁定 URL 格式
- `sb.rs` 仅依赖 reqwest + tokio，不依赖业务模块（便于单独 mock）
- `routes.rs` 是唯一组合点，main.rs 通过它挂载所有 API

---

## 4. 数据契约（来自设计文档 §7，本仓库为消费者）

### 4.1 devices.json（端侧写，BFF 读）

路径：`serv/opencode/{sb_user}/devices.json`

```json
{
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
}
```

**本仓库实现**：`devices.rs::load_devices()` GET 一次，TTL 30s 内存缓存。

### 4.2 serv/web/{pcname}/config.md（门户用户配置）

```json
{
  "version": 1,
  "pcname": "samuel-mac",
  "updated_at": "2026-08-28T11:00:00+08:00",
  "jump_overrides": [
    { "device": "macos/samuel",  "base_url": "http://192.168.1.5:9464" }
  ],
  "default_base_url": "",
  "manual_devices": [
    { "id": "local-test", "label": "本机测试", "base_url": "http://127.0.0.1:9464" }
  ]
}
```

**本仓库实现**：`config_md.rs::read_config(pcname)` / `write_config(pcname, config)`，GET-merge-PUT 防丢字段。

### 4.3 端侧 API（设计文档 §1.1，本仓库通过 proxy.rs 消费）

- `GET /health`（探活）
- `GET /project`（Basic 鉴权）
- `GET /session?directory={dir}`（Basic 鉴权）
- `POST /api/session`（Basic 鉴权，创建会话）

---

## 5. API 实现矩阵（设计文档 §11）

| 方法   | 路径                                          | 实现模块                                | 鉴权       |
| ------ | --------------------------------------------- | --------------------------------------- | ---------- |
| POST   | `/api/login`                                    | auth.rs                                 | 无         |
| POST   | `/api/logout`                                   | auth.rs                                 | Cookie     |
| GET    | `/api/devices`                                  | devices.rs + proxy.rs（探活）           | Cookie     |
| GET    | `/api/config?pcname=`                           | config_md.rs                            | Cookie     |
| PUT    | `/api/config?pcname=`                           | config_md.rs                            | Cookie     |
| GET    | `/api/devices/{pctype}/{pcname}/projects`       | proxy.rs（在线）/ path-list（离线）     | Cookie     |
| GET    | `/api/devices/{pctype}/{pcname}/sessions`       | proxy.rs                                | Cookie     |
| POST   | `/api/devices/{pctype}/{pcname}/sessions`       | proxy.rs                                | Cookie     |
| POST   | `/api/manual/{device_id}/sessions`              | proxy.rs（config_md.manual_devices）    | Cookie     |
| GET    | `/api/devices/{pctype}/{pcname}/jump`           | jump.rs（302 / `?format=json`）         | Cookie     |
| GET    | `/healthz`                                      | main.rs                                 | 无         |

**错误统一格式**：`{"error":{"code":"...","message":"..."}}`

**白名单校验**：
- `pctype`：仅接受 `macos|windows`
- `pcname`：白名单字符 `[A-Za-z0-9_-]{1,64}`
- `device_id`（manual）：同 pcname 白名单
- `directory`：percent-encoding 透传给 oc

---

## 6. 关键算法

### 6.1 jump URL 生成（jump.rs::build_jump_url）

```
base_url = 解析优先级（§7.2）:
           config.md.jump_overrides[device] > config.md.default_base_url > devices.json.public_url

jump_url = {base_url}/{base64url_no_pad(directory)}/session/{session_id}
```

- base64 使用 URL-safe 无填充变体（`-`/`_`，去除 `=`）
- `directory` 原始字节先 percent-encode（中文路径安全）
- 单测覆盖：普通 ASCII / 中文路径 / 含 `/` 路径 / 优先级 4 组用例

### 6.2 base_url 校验（jump.rs::validate_base_url）

- 必须 `http://` 或 `https://` 开头
- 仅保留 `scheme://host:port`，拒绝路径部分
- 拒绝 `javascript:` / `data:` / 空 host
- host 可为 IP 或域名

### 6.3 pcname 白名单（config_md.rs::validate_pcname）

- 正则 `[A-Za-z0-9_-]{1,64}`
- 拒绝空字符串、超长、特殊字符

### 6.4 Cookie HMAC（auth.rs）

- HMAC-SHA256 签名，密钥首次启动随机生成（32 字节），持久化到 `/data/cookie_key`（0600）
- HttpOnly + Secure + SameSite=Lax
- 有效期 7 天，滑动续期（每次访问后延长 7 天）

### 6.5 IP 限速（auth.rs）

- 按 IP 失败计数：5 次 / 10 分钟，内存计数
- 仅对 `/api/login` 生效
- 单实例内存限速（设计文档明确：多实例时再接入 Redis）

---

## 7. 服务器部署文件增量（输出路径与最小 diff 描述）

### 7.1 `/Users/samuel/Documents/Server/8.159.159.138/docker-compose.yml`

追加 mini-oc-web 服务块（设计文档 §12.1 完整照搬，命名替换 `mini-co-web` → `mini-oc-web`）：

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
      - OPENCODE_SERVER_PASSWORD=<统一账密>
      - SB_BASE_URL=http://silverbullet:3000
      - SB_USER=admin
      - SB_PASSWORD=<SB密码>
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

### 7.2 `/Users/samuel/Documents/Server/8.159.159.138/nginx/nginx.conf`

追加：
- upstream `mini_oc_web`（§12.3）
- server block 80（web.isoops.com → https rewrite）
- server block 443（web.isoops.com → `mini_oc_web` upstream）

证书复用 `ssl_isoops.pem/key`（前提：已确认证书覆盖此域名 —— 设计文档 §19 开放问题 #3）。

### 7.3 部署验证命令

```bash
docker compose config                          # compose 语法校验
docker compose exec nginx nginx -t             # nginx 配置校验
docker compose up -d mini-oc-web
docker compose logs -f mini-oc-web             # 启动日志
curl -I https://web.isoops.com/healthz         # 期望 200
```

---

## 8. SPA（占位骨架）

**目标**：路由 + API 客户端骨架完整，业务页可点击可跳转。具体 UI 留后续 PR。

### 8.1 路由

```
/login                              → LoginPage.vue
/devices                            → DevicesPage.vue
/devices/:pctype/:pcname/projects   → ProjectsPage.vue
/devices/:pctype/:pcname/sessions   → SessionsPage.vue（query: ?dir=）
/settings                           → SettingsPage.vue
```

### 8.2 API 客户端（web/src/api.ts）

```ts
// 伪代码示意
export async function apiGet(path: string): Promise<any>
export async function apiPost(path: string, body: any): Promise<any>
export async function apiPut(path: string, body: any): Promise<any>
// 401 自动跳 /login
// 错误统一从 {error:{code,message}} 提取
```

### 8.3 占位页契约

每个占位页：标题 + 「调用 API + 渲染结果」最小示例，足以验证 BFF 契约。

---

## 9. 关键技术约束（来自设计文档 / §3.2 / §9 / §14）

| 约束             | 措施                                                          |
| ---------------- | ------------------------------------------------------------- |
| CORS 白名单      | 所有设备调用由 BFF 服务端出站（容器内 HTTPS），浏览器只同域    |
| 深链 sidebar     | jump.rs 集中生成 URL，单测锁定，oc 升级时一处适配              |
| Basic 仅弹窗     | 不在前端 / URL / devices.json / config.md 存账密              |
| 中文路径安全     | `directory` percent-encoding 透传                            |
| 注入防御         | `pctype/pcname/device_id` 路径参数白名单校验                  |
| 开放重定向防御   | jump 目标 = devices.json ∪ config.md 并集校验，拒任意 URL      |
| 镜像精简         | `reqwest` 启用 `rustls-tls` feature（无需 openssl）           |
| 配置零密钥       | 容器 env 0600；SB 不落密码；前端仅持 pcname（localStorage）   |

---

## 10. 验证策略

每完成一个里程碑必须产出对应证据：

| 里程碑           | 验证命令                                          | 通过判据                       |
| ---------------- | ------------------------------------------------- | ------------------------------ |
| jump.rs TDD       | `cargo test -p mini-oc-web jump`                 | 全绿，覆盖 4 组用例             |
| 全部单测         | `cargo test --lib`                                 | 全绿，无 panic、无 unwrap 漏掉 |
| BFF 集成测试     | `cargo test --test bff_integration`                | 全绿（mock 设备：在线/401/超时） |
| 本地运行         | `cargo run` + `curl http://127.0.0.1:8100/healthz` | 返回 200                       |
| 镜像构建         | `docker build .`                                  | 多阶段成功，镜像 < 200MB       |
| compose 语法     | `cd /Users/samuel/Documents/Server/8.159.159.138 && docker compose config` | 无错误                          |
| nginx 语法       | `docker compose exec nginx nginx -t`             | syntax is ok                   |
| LSP 检查         | `lsp_diagnostics` on 改动的 .rs 文件             | 无 error / warning             |

---

## 11. 实施阶段划分（基于设计文档 §17 Phase 1）

| 阶段 | 内容                                                                    | 交付判据                                       |
| ---- | ----------------------------------------------------------------------- | ---------------------------------------------- |
| T0   | 仓库初始化（Cargo.toml / .gitignore / .env.example / README）           | `cargo check` 通过                              |
| T1   | jump.rs TDD（base64url / 中文 / 校验 / 优先级）                          | `cargo test jump` 全绿                          |
| T2   | config_md.rs / auth.rs / sb.rs 纯逻辑模块 TDD                            | `cargo test --lib` 全绿                          |
| T3   | devices.rs + proxy.rs（带 reqwest，需要 mock 测试）                       | 集成测试全绿                                     |
| T4   | routes.rs 组装 + main.rs 启动                                            | `cargo run` + curl `/healthz` 通过              |
| T5   | Dockerfile 多阶段构建 + .dockerignore                                    | `docker build .` 成功                           |
| T6   | docker-compose.yml 本地自包含（mock SB 可选）                            | `docker compose up` + `/healthz` 通过           |
| T7   | 服务器部署文件增量（compose / nginx）+ 部署验证                          | `docker compose config` + `nginx -t` 通过       |
| T8   | SPA 占位骨架（web/ 目录 + 5 个占位页 + README）                          | `npm install && npm run build` 成功              |
| T9   | 端到端验收脚本（README 中描述）                                          | 文档完整                                        |

---

## 12. 风险与对策（继承设计文档 §16 + 本仓库新增）

| 风险                              | 等级 | 对策                                            |
| --------------------------------- | ---- | ----------------------------------------------- |
| oc 路由格式变更                   | 中   | jump.rs 集中生成，单测锁定                       |
| SB 容器不可达                     | 中   | 内存 stale 缓存 + 进程内兜底（设计文档 §14）      |
| 端侧未上线 devices.json           | 中   | 设备列表为空 + UI 提示，**不报错**                |
| PCNAME 字符绕过                   | 低   | 白名单 + 单测锁定                                |
| 跳转目标被替换为恶意 URL          | 低   | jump.rs 校验 + 白名单并集                        |
| Docker DNS 解析延迟（容器重启）   | 低   | nginx `resolve;` + `resolver 127.0.0.11`（已有）  |
| SPA 完整业务留后续 PR 导致门户裸奔 | 中   | T8 占位骨架 + README 明确标注「业务未实现」         |

---

## 13. ADR（架构决策记录）

| #  | 决策                                                                | 理由                                                            |
| - | ------------------------------------------------------------------- | --------------------------------------------------------------- |
| 1 | BFF 模式（服务端代理设备调用）                                       | 规避 oc CORS；账密不出服务端                                    |
| 2 | `mini-co-web` → `mini-oc-web` 统一命名                              | 与本仓库目录名一致                                              |
| 3 | SPA 仅占位骨架                                                     | 业务逻辑留后续 PR，本会话聚焦 BFF + 契约                          |
| 4 | TDD 节奏（纯逻辑先测后实现）                                         | jump URL / 校验 / 白名单三处是关键逻辑，必须锁定                   |
| 5 | 服务器部署文件直接 patch `/Users/samuel/Documents/Server/.../`      | 用户接受范围已包含；不进入本仓库 commit                            |
| 6 | Rust + Axum + Tokio + reqwest（rustls-tls）                          | 与设计文档一致；镜像无需 openssl                                  |
| 7 | Vue 3 + Vite + Pinia + Vue Router                                  | 生态成熟 + 移动端组件库丰富 + 构建产物轻量                       |
| 8 | 多阶段 Dockerfile（rust + node）                                    | SPA 产物内嵌，无需额外挂载                                      |

---

## 14. 后续 PR 范围（不在本会话）

1. mini-oc-gui 端侧改造（设计文档 §8 全 4 项）
2. SPA 完整业务页面（5 个页面的 UI + 交互 + 状态管理）
3. Phase 2 iframe 预热跳转（消除深链回落）
4. Phase 2 path-list sections 元数据升级（离线真实标题）
5. Phase 3 nginx `auth_request` 全域 SSO
6. Docker 镜像推送 harbor（harbor.nnkcy.com/open-library/mini-oc-web）