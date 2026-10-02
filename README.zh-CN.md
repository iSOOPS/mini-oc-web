# mini-oc-web

[English](README.md) | **简体中文**

一个自托管的 Web 门户，用于跨设备浏览 [OpenCode](https://opencode.ai) 会话。

mini-oc-web 是一个用 Rust（Axum）编写的 **BFF（Backend-For-Frontend）**，配套 Vue 3 单页应用（SPA）：它聚合设备状态、把项目/会话 API 服务端代理到各设备的 `oc serve` 实例，并生成安全的深链（deep-link）跳转 URL —— 让你在单个页面上查看并恢复任何机器（macOS / Windows）上的编码会话，用手机访问也没问题。

## 架构总览

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

- **设备端**（`mini-oc-gui`，独立项目）：在本地运行 `opencode serve` 与 [rathole](https://github.com/rapiz1/rathole) 隧道客户端，把本地 oc 服务经由云端暴露为 `{cloud_ip}:{port}`。
- **SilverBullet** 作为事实源：一份 `users.json` 形状的注册表文档保存租户、登录密钥、设备授权以及每用户的存储配置。
- **BFF** 负责用户认证、读取注册表、探测每台设备的 `/status`、在服务端代理项目/会话调用，并构造深链 URL。

## 核心设计指南

### 1. BFF 模式：所有设备调用都发生在服务端

浏览器只与门户源（origin）通信。对设备的每一次出站调用（探测、项目/会话代理）都由 BFF 自己完成。这带来：

- 完全绕开 `oc serve` 的 CORS 白名单；
- HTTP Basic 凭据不会落入前端、URL 或日志；
- SPA 保持为纯粹的同源消费者。

### 2. 认证：HMAC 签名 Cookie + 登录限流

- Cookie 值以 HMAC-SHA256 签名，密钥在首次启动时随机生成并持久化到 `COOKIE_KEY_PATH`（权限 0600）。
- Cookie 标志：`HttpOnly + Secure + SameSite=Lax`；有效期 7 天，滑动续期。
- 按来源 IP 的登录限流：10 分钟内失败 5 次 → `429 rate_limited`（内存实现；设计上就是单实例，真要多实例才考虑 Redis）。
- 独立的管理控制台（`/admin`），有自己的 Cookie 会话与超级管理员密码。

### 3. 多租户用户注册表（以 SilverBullet 作为存储）

SilverBullet 中的 `web/opencode/config.md` 持有一份 `users.json` 形状的注册表：

```json
{
  "version": 1,
  "users": [
    {
      "id": "100001",
      "name": "alice",
      "key": "<32位登录密钥>",
      "cloud_ip": "<rathole 服务器 ip>",
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

- 用户使用 `name + key` 登录（key 同时也是跳转到设备的凭据）。
- 设备是"授权项"：管理员分配隧道服务名 + 端口；设备之后通过 `POST /api/device-bind` 自行绑定（写入 `device-name`，把 `bound` 翻转为 true）。
- `public-url` 是设备穿透地址 `http(s)://host[:port]` —— 允许自带端口（`ip:port` 隧道端点），带端口时直接按该地址访问。`oc-port` 可空：非空时浏览器跳转 URL 以路径前缀拼接（`{public-url}/{oc-port}/…`），为空时跳转地址即穿透地址本身。
- 不做缓存：每个请求都直接从 SilverBullet 读取注册表 —— 用户量小，TTL 缓存（以及它带来的陈旧性 bug）不值得（ADR #20/#21）。

### 4. 设备状态：一次往返 —— `/api/me`

`GET /api/me` 返回用户的设备列表，**并附带运行时状态**：BFF 并发探测每台设备的 TUI 服务地址（每台 4 秒超时，`futures::join_all`，分批执行）—— 无端口穿透地址拼 `{public-url}:{port}/status`，自带端口的直接用 `{public-url}`。探测永远不会导致请求失败 —— 掉线的设备只会以 `online:false, reason:"…"` 的形式返回，SPA 依据 `available ∈ {-1, 0, 1}` 渲染红/橙/绿状态点。

### 5. 深链跳转 URL（base64url，覆盖优先级）

```
{base_url}/{base64url_nopad(directory)}/session/{session_id}[?auth_token=…]
```

- `directory` 以无填充的 URL-safe base64 编码，保证中文、带空格的路径在每一跳都安然无恙；该格式由单元测试锁定（`jump.rs`，零依赖）。
- `base_url` 优先级：设备级覆盖 > 用户配置 > 注册表 `public-url`
  按 `{public-url}[/{oc-port}]` 组合（路径前缀；`oc-port` 为空时省略该段）。
  覆盖值只改变**浏览器**跳转的目标地址；BFF 数据面永远走设备的 TUI
  穿透地址（服务器够不到用户的局域网）。
- `auth_token`（`user:pass` 的 base64）只附加在局域网回退链接上，让用户免去浏览器原生 Basic 认证弹窗。
- 防开放重定向：跳转目标必须来自注册表/配置的并集 —— 任意 URL 一律拒绝。

### 6. 加固的输入处理

- 路径参数（`pctype`、`pcname`、`device_id`）过白名单校验（`pctype ∈ {macos, windows}`，`pcname` 匹配 `[A-Za-z0-9_-]{1,64}`）。
- `directory` 做百分号编码后原样透传。
- `base_url` 必须形如 `http(s)://host[:port]` —— 不允许携带路径，禁止 `javascript:`/`data:`。
- 统一错误信封：`{"error":{"code":"…","message":"…"}}`（见下表）。

### 7. oc-serve 线格式适配（代理层）

`proxy.rs` 在服务端归一化 opencode 原生 API 的形状，让 SPA 消费干净的契约：

| BFF 契约 | oc-serve 原生形状 |
|---|---|
| 创建会话请求 `{directory, title}` | `{title, location: {directory}}` |
| 创建会话响应 `{id, directory}` | `{data: {id, title, directory}}` |
| 项目列表 `[{path, lastOpenedAt}]` | `[{id, path, name, sessions: […]}]` |
| 会话列表 `[{id, title, updatedAt}]` | `[{id, title, directory, created_at, updated_at}]` |

### 8. 架构决策记录（ADR 1–23）

| # | 决策 | 原因 |
|---|------|------|
| 1 | BFF 代理所有设备流量 | 绕开 CORS；凭据保留在服务端 |
| 2 | 更名 `mini-co-web` → `mini-oc-web` | 命名与仓库对齐 |
| 3 | SPA 先以骨架落地 | 完整 SPA 逻辑留待后续 PR |
| 4 | 纯逻辑模块先行 TDD | jump / 校验 / 白名单是关键路径 |
| 5 | 服务器部署文件以补丁形式放用户管理目录 | 不进本仓库 |
| 6 | Rust + Axum + reqwest（rustls-tls） | 静态产物小，不依赖 OpenSSL |
| 7 | Vue 3 + Vite + Pinia + vue-router | 产物轻量，生态成熟 |
| 8 | 多阶段 Dockerfile（node + rust） | SPA 内嵌进镜像，无需挂载 |
| 9 | 手动建会话用 `?pcname=` 查询参数定位设备 | 前端 store 不存 pcname |
| 10 | 注册表写入只用 POST，绝不用 GET | GET 不得改服务端状态 |
| 11 | `/jump` 失败时 SPA 回退本地 base64 | 与 oc 侧行为一致；不阻塞 |
| 12 | 集成测试用 `axum-test` + `wiremock` | 复用现有依赖，不引 `mockito` |
| 13 | 健康探测发送零凭据 | 明确"无需认证"语义；日志不落假凭据 |
| 14 | 前端 `Promise.allSettled`，批次 ≤ 6 | 实际 N ≪ 6；Chrome 同源连接上限 6 |
| 15 | LoadingOverlay 传送至 `<body>` | 摆脱父级 z-index/overflow/transform 上下文 |
| 16 | 取消按钮仅在 cancelable 时渲染 | 一个组件服务两种加载形态 |
| 17 | 首次加载静默；显式刷新才出转圈 | 首屏闪转圈是坏体验 |
| 18 | 取消操作用 toast 确认 | 取消是用户动作，需要反馈 |
| 19 | `/api/me` 一次调用返回设备 + 运行时状态 | 消灭 N+1 刷新模式 |
| 20 | 不对 SilverBullet 用户数据做 TTL 缓存 | 这个规模下，陈旧性 bug 的代价 > 性能收益 |
| 21 | 不对设备数据做 TTL 缓存 | 同上 |
| 22 | 保留 `/api/device-status/:port` | 为将来单设备深度探测预留 |
| 23 | BFF 在探测期间忽略客户端中断 | reqwest 的取消管线不值得做 |

## 仓库结构

```
mini-oc-web/
├── Cargo.toml                # bin + lib (mini_oc_web)
├── src/
│   ├── main.rs               # 引导：tracing、AppState、路由、监听器
│   ├── state.rs              # AppState::from_env —— 配置 + 共享客户端
│   ├── routes.rs             # 所有 /api/* 处理器 + 静态 SPA 服务
│   ├── auth.rs               # 登录/登出、HMAC Cookie、按 IP 限流
│   ├── users.rs              # 租户注册表（加载/保存/校验、密钥生成）
│   ├── sb.rs                 # SilverBullet HTTP 客户端（cookie 名推导）
│   ├── probe.rs              # 设备 /status 探测（永不报错）
│   ├── proxy.rs              # oc-serve API 客户端（项目/会话/创建）
│   ├── jump.rs               # 深链 URL 构造器（纯函数，单元测试锁定）
│   └── error.rs              # AppError → HTTP 状态码 + 错误信封
├── web/                      # Vue 3 SPA（Vite + TS + Pinia）
│   └── src/
│       ├── pages/            # 登录、设备、项目、会话、管理后台…
│       └── components/       # 对话框、加载遮罩
└── tests/                    # 集成测试套件（wiremock/模拟）
```

模块规则：依赖单向、无循环；`jump.rs` 是零依赖的纯逻辑；`sb.rs` 只依赖 reqwest/tokio；`routes.rs` 是唯一的组合点。

## API 参考

| 方法 | 路径 | 认证 | 用途 |
|--------|------|------|---------|
| POST | `/api/login` | — | 用户登录（name + key）→ 会话 Cookie |
| POST | `/api/logout` | cookie | 清除会话 |
| GET | `/api/me` | cookie | 当前用户 + 设备列表**（含运行时状态）** |
| GET | `/api/me/key` | cookie | 重新显示自己的登录密钥 |
| POST | `/api/me/name` | cookie | 重命名自己 |
| POST | `/api/me/sb` | cookie | 更新自己的 SilverBullet 连接配置 |
| POST | `/api/user/info` | key | 设备端获取所属用户的记录 |
| POST | `/api/device-bind` | key | 设备自行注册（`device-name`、`bound`） |
| GET | `/api/device-status/:port` | cookie | 单设备深度探测 |
| GET | `/api/devices/:pctype/:pcname/projects` | cookie | 代理：项目列表 |
| GET | `/api/devices/:pctype/:pcname/sessions` | cookie | 代理：会话列表 |
| POST | `/api/devices/:pctype/:pcname/sessions` | cookie | 代理：创建会话 |
| GET | `/api/devices/:pctype/:pcname/detail` | cookie | 设备连接详情（Basic 凭据） |
| GET | `/api/devices/:pctype/:pcname/jump` | cookie | 构造深链（302 或 `?format=json`） |
| POST | `/api/admin/login` / `/api/admin/logout` | — / admin | 管理员会话 |
| GET | `/api/admin/info` | admin | 运行时信息（绑定、SB 状态、运行时长…） |
| GET | `/api/admin/users` | admin | 列出租户 |
| POST | `/api/admin/users` | admin | 创建租户（自动生成 key） |
| PUT | `/api/admin/users/:id` | admin | 更新租户 |
| POST | `/api/admin/users/:id/regenerate-key` | admin | 重置登录密钥 |
| GET | `/healthz` | — | 存活探测 |

错误码（统一信封 `{"error":{"code","message"}}`）：

| 错误码 | HTTP | 含义 |
|---|---|---|
| `unauthorized` | 401 | Cookie 缺失或无效 |
| `invalid_pcname` | 400 | `pcname` 未通过白名单（`[A-Za-z0-9_-]{1,64}`） |
| `invalid_target` | 400 | `pctype` 不在 `{macos,windows}` 内，或跳转目标未通过校验 |
| `not_found` | 404 | 设备 / 会话 / 路径不存在 |
| `rate_limited` | 429 | 同一 IP 10 分钟内登录失败 5 次以上 |
| `device_auth_failed` | 502 | 设备返回 401/403（凭据不匹配） |
| `device_offline` | 502 | 设备不可达（连接失败/超时） |
| `service_unavailable` | 503 | SilverBullet 不可达 |
| `internal` | 500 | 其他服务端错误（解析失败等） |

## 快速开始

```bash
# 克隆仓库
git clone <repository-url>
cd mini-oc-web

# 复制环境配置
cp .env.example .env
# 编辑 .env：设置 SB_PASSWORD + SB_BASE_URL

# 构建 SPA（一次性；也可以交给 Docker 多阶段构建）
cd web && npm install && npm run build && cd ..

# 运行服务
cargo run

# 浏览器打开
open http://127.0.0.1:8100
```

**没有 SilverBullet（仅本地开发）：** 设置 `SB_BASE_URL=http://127.0.0.1:65535`（不可达）。BFF 会正常启动（打印警告），依赖注册表的接口返回 `503 service_unavailable`，而 `/healthz` 与 `/`（SPA）照常工作。

## 配置

所有配置来自环境变量（见 [`.env.example`](.env.example)）：

| 变量 | 默认值 | 用途 |
|---|---|---|
| `WEB_PORT` / `WEB_BIND` | `8100` / `0.0.0.0` | HTTP 监听地址 |
| `COOKIE_KEY_PATH` | `/data/cookie_key` | HMAC 密钥文件（自动生成，0600） |
| `WEB_STATIC_DIR` | `./web/dist` | 构建后的 SPA 目录 |
| `SB_BASE_URL` | `http://127.0.0.1:3000` | SilverBullet 端点 |
| `SB_USER` / `SB_PASSWORD` | — | SilverBullet 账号（密码同时作为管理控制台密码） |
| `PORTAL_BASE` | `http://127.0.0.1:8100` | 构造深链 URL 使用的对外源 |

## Docker

```bash
# 构建 Docker 镜像（多阶段：node 构建 SPA + rust release 构建）
docker build -t mini-oc-web .

# 用 docker compose 运行（自成一体：桥接网络 + 数据卷）
docker compose up -d
```

部署注意事项：

- HMAC Cookie 密钥存放在 `mini-oc-web-data` 具名卷（`/data`）中。
  丢失它会让所有会话失效 —— 务必保持该卷持久化。
- 正式部署前必须替换 `changeme` 默认值（`SB_PASSWORD`）。

生产环境请自行将门户置于反向代理（nginx/Caddy 等）之后：单一域名，把 `/{b64pc}/…` 形式的设备深链前缀分发到对应的 rathole 上游 —— 部署配置由仓库外自行管理。

## 测试

```bash
cargo test          # 单元 + 集成测试套件（wiremock/模拟，无需真实设备）
cd web && npm run build   # vue-tsc 类型检查 + vite 构建
```

## 路线图

- **阶段 2** —— iframe 深链预热（消除冷启动回退）、路径列表区块元数据升级
- **阶段 3** —— nginx `auth_request` 全域 SSO

## 许可证

[MIT](LICENSE)
