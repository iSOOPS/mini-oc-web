# mini-co-web 设计文档（Web/H5 会话门户）

**Status:** Draft v2（2026-08-28，替代同名 v1）
**Scope:** 独立项目 mini-co-web（服务器 Docker 部署）+ 端侧（mini-oc-gui）最小改造 + 云端设备注册表与门户配置存储
**服务器实况依据**：`/Users/samuel/Documents/Server/8.159.159.138`（docker-compose / nginx / rathole / silverbullet 已核实）

---

## 1. 背景与目标

### 1.1 现有系统

`mini-oc-gui-serve`（本仓库）是部署在 win / macOS 各台电脑上的 Rust 端侧工具：

- TUI 启停 `opencode serve`（本机 127.0.0.1:9464）与 `rathole` 隧道客户端；
- rathole 连接服务器（8.159.159.138），每台设备一个 service → 服务器 rathole-server 容器一个服务端口 → nginx 域名反代，实现公网域名直达本机 oc web；
- `path-list.md`（项目 ↔ 会话索引）本地缓存 + SilverBullet（md.isoops.com）云端同步，按
  `serv/opencode/{sb_user}/{pctype}/{pcname}/path-list.md` 命名空间隔离；
- 端侧暴露 HTTP API（`GET /project`、`GET /session?directory=`、`POST /api/session`、`GET /health`），与 oc serve 共用同一组 HTTP Basic 账密。

### 1.2 目标

新增 **mini-co-web**：独立项目（独立仓库，后续人工建立），部署在服务器 Docker 中，提供 Web/H5 门户：

1. 跨设备（win / macOS）选择；
2. 每台设备下的项目列表、每个项目下的 oc 会话列表（真实标题）；
3. 新建会话；
4. 点击会话后 **302 跳转直达 oc web 对应会话**；
5. **Web 端可配置跳转地址**（指定 ip + port），配置以 md 文件存云端 `serv/web/{pcname}/config.md`，便于本地 / 局域网测试时绕过公网隧道直连。

### 1.3 非目标

- 不做 oc / rathole 的启停管理 —— 仍归属各端 TUI；
- 不代理 / 内嵌 oc web 的对话界面（跳转后由 oc web 原生承载）；
- 不引入新的用户体系（复用统一账密，见 §9）。

---

## 2. 术语表

| 术语                  | 含义                                                                            |
| --------------------- | ------------------------------------------------------------------------------- |
| 设备 (device)         | 一台运行 mini-oc-gui-serve 的 win / macOS 电脑，由 `(pctype, pcname)` 唯一标识    |
| mini-co-web           | 本设计的独立门户项目（BFF + SPA），Docker 部署于服务器                              |
| 端侧                  | mini-oc-gui-serve（现有仓库）                                                      |
| SB                    | SilverBullet（md.isoops.com / 容器 silverbullet:3000），HTTP 文件存储               |
| 统一账密              | `OPENCODE_SERVER_USERNAME/PASSWORD`，同时保护端侧 API 与 oc serve                  |
| 深链 (deep link)      | 直达 oc web 某会话的 URL：`/{base64(directory)}/session/{session_id}`              |
| 跳转地址 (base_url)   | 某设备的 oc web 根地址（公网域名或 ip:port 直连地址）                              |
| override              | `serv/web/{pcname}/config.md` 中对某设备跳转地址的用户级覆盖                        |

---

## 3. 现状盘点与关键约束

### 3.1 服务器现状（已核实，mini-co-web 的部署基座）

| 组件                | 实况                                                                                                       |
| ------------------- | ---------------------------------------------------------------------------------------------------------- |
| docker-compose      | 自定义 bridge 网络 `app-net`，服务间以服务名互通；镜像统一来自私有库 `harbor.nnkcy.com/open-library/*`        |
| nginx               | 容器映射宿主机 80/443，挂载 `nginx/nginx.conf` + `certs/`；80 全部 rewrite → https；证书 `ssl_isoops.pem/key` |
| rathole-office-mac  | 控制口 2333（映射宿主机），服务口 **12333**（仅容器网络内 expose）→ 办公室 Mac 的 oc（9464）                 |
| rathole-home-win    | 控制口 3333（映射宿主机），服务口 **13333**（仅容器网络内 expose）→ 家里 Win 的 oc（9464）                   |
| silverbullet        | `SB_USER=admin:<password>`，容器网络 `silverbullet:3000`（expose）；公网域名 `md.isoops.com`                   |
| redis               | 已有（requirepass），本设计**暂不使用**（单实例内存限速即可，多实例时再接入）                                  |
| 域名 → upstream      | `oc-mac.isoops.com → rathole_office_mac(12333)`、`oc-win.isoops.com → rathole_home_win(13333)`、`md.isoops.com → silverbullet_local(3000)` |
| nginx upstream 写法  | `server <服务名>:<端口> resolve;` + `resolver 127.0.0.11`（Docker 内置 DNS，容器重启换 IP 后自动重解析）        |

### 3.2 外部约束（已对照 sst/opencode 源码核实）

| # | 约束                        | 来源                                                                                          | 对策章节 |
| - | --------------------------- | --------------------------------------------------------------------------------------------- | -------- |
| 1 | **CORS 白名单**：oc serve 默认仅放行 `localhost:*` / `127.0.0.1:*` / `tauri://localhost` / `https://*.opencode.ai`，自定义需 `--cors` | `packages/opencode/src/server/server.ts`                       | §6 BFF    |
| 2 | **深链 sidebar 限制**：直接 URL 导航 `/:dirBase64/session/:sessionId` 仅当该项目已在该浏览器 localStorage sidebar 中时直达，否则回落会话列表页 | `packages/app/src/pages/layout.tsx`                            | §10       |
| 3 | **鉴权仅 HTTP Basic 弹窗**：oc web 无 token / cookie 登录，不读 URL 参数或 localStorage 凭据 | `server.ts` 的 `basicAuth({username, password})`                | §9        |

oc web 另提供 iframe 深链协议（`packages/app/src/pages/layout/deep-links.ts`）：
`opencode://open-project?directory=<path>`、`opencode://new-session?directory=<path>&prompt=<text>` —— Phase 2 用于预热跳转（§10.4）。

---

## 4. 总体架构

```
┌─────────────────────────┐
│  手机 / PC 浏览器 (h5)   │  ① 登录门户 → 浏览设备/项目/会话
└───────────┬─────────────┘
            │ https://web.isoops.com (门户 Cookie 会话)
            ▼
┌──────────────────────────────────────────────┐      ┌──────────────────────┐
│ 服务器 8.159.159.138 (docker app-net)         │      │ SilverBullet          │
│  ┌────────────────────────────────────┐      │      │ md.isoops.com         │
│  │ mini-co-web (新增容器, BFF + SPA)    │ ─────┼─GET──│ devices.json (新增)   │
│  │  登录/聚合/代理/跳转URL生成/配置管理   │ ─────┼─GET──│ serv/web/{pcname}/   │
│  └──────────┬─────────────────────────┘      │      │   config.md (新增)    │
│             │ ② GET /health 探活              │      │ 各机 path-list.md     │
│             │    GET /project (Basic)         │      │ （离线兜底）           │
│             │    GET oc:/session?dir (Basic)  │      └───────▲──────────────┘
│             ▼ (容器内出站调用, 无 CORS 问题)    │      ┌───────┴──────────────┐
│  ┌────────────────────────────────────┐      │      │ 各端设备 (现有)        │
│  │ nginx (增量配置)                     │      │      │ macOS: mini-oc-gui    │
│  │  web.isoops.com  → mini-co-web:8100  │      │      │   + oc serve :9464    │
│  │  oc-mac.isoops.com → rathole:12333   │◄─────┼──────│ Win: mini-oc-gui      │
│  │  oc-win.isoops.com → rathole:13333   │◄─────┼──────│   + oc serve :9464    │
│  └────────────────────────────────────┘      │      └──────────────────────┘
└──────────────────────────────────────────────┘
            ④ 用户点「进入会话」→ 302 跳转
               默认: https://oc-mac.isoops.com/{base64(dir)}/session/{ses_id}
               override: http://192.168.x.x:9464/{base64(dir)}/session/{ses_id}  ← 本地/局域网测试
               (浏览器整页导航直连, Basic 弹窗一次, 无 CORS 问题)
```

**新增组件**：① mini-co-web 容器（独立仓库）；② 云端 `devices.json`（设备注册表）；③ 云端 `serv/web/{pcname}/config.md`（门户用户配置）。
**零改动组件**：rathole、nginx 现有 server 块、oc serve 启动方式、path-list.md 现有同步机制。

---

## 5. 项目与仓库形态

**结论：mini-co-web 为完全独立项目、独立仓库（后续人工建立，建议同名 GitHub 私有仓库）。**

| 维度           | 说明                                                                                                                                                 |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| 独立仓库       | 服务器侧门户与端侧工具的开发节奏、部署目标（Linux Docker vs win/macOS 本机）、构建约束（门户无 no-C-toolchain 限制，`reqwest` 可直接用 rustls-tls）完全不同                 |
| 契约同步       | `devices.json` / `path-list.md` / `serv/web/{pcname}/config.md` 三份数据契约由端侧（写）与门户（读）共同消费 —— **以本文档 §7 为契约规范**，两端仓库各自实现解析，schema 变更需双向同步（见 §16 风险） |
| 代码复用       | 端侧 `RemoteClient`（SB HTTP 客户端）等逻辑在 mini-co-web 中**重新实现**（约 200 行：登录 → cookie → GET/PUT `/.fs/`），不跨仓库引用；实现语言仍推荐 Rust + Axum，也可按团队熟悉度换栈（Node/Go），契约不变 |
| 本仓库改动     | 仅 §8 的端侧改造（设置项 + devices.json 上报），在 mini-oc-gui 仓库内完成                                                                               |

### mini-co-web 仓库结构（建议）

```
mini-co-web/                 # 独立仓库（人工建立）
├── Cargo.toml               # bin: mini-co-web
├── Dockerfile               # 多阶段构建（§12.2）
├── docker-compose.yml       # 本地/服务器自包含运行（可选）
├── .env.example             # WEB_PORT / 统一账密 / SB 配置
├── src/
│   ├── main.rs
│   ├── auth.rs              # 登录 + Cookie 会话（HMAC 签名）+ 限速
│   ├── sb.rs                # SB 客户端（login/cookie/GET/PUT /.fs/）
│   ├── devices.rs           # devices.json 读取缓存
│   ├── config_md.rs         # serv/web/{pcname}/config.md 读写
│   ├── proxy.rs             # 设备 API 出站调用（探活/project/session/创建）
│   ├── jump.rs              # 跳转 URL 生成（base64url 深链, 单测锁定）
│   └── routes.rs            # /api/* 路由 + 静态资源服务
└── web/                     # SPA 源码（构建产物内嵌或 volume 挂载）
    └── dist/
```

---

## 6. 数据获取：BFF 模式（实时为主、云端兜底）

所有对**公网设备**的调用由 mini-co-web **服务端**发起（容器内出站 HTTPS，绕开 §3.2-1 的 CORS 约束），浏览器只与门户同域交互。**不在浏览器向设备域名发起任何 XHR/fetch。**

| 数据                   | 在线路径（优先）                                                                                                        | 离线兜底                                    |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| 设备列表               | `devices.json`（GET 一次，TTL 30s 内存缓存）                                                                              | 同左（静态文件）                             |
| 设备在线状态           | 并行 `GET {public_url}/health`（免鉴权，3s 超时）；仅在线设备继续后续查询                                                  | 显示「离线」，列表仍可浏览（兜底数据）        |
| 项目列表               | `GET {public_url}/project`（Basic 统一账密）→ 真实 path + lastOpenedAt                                                     | GET 该机命名空间 `path-list.md`              |
| 会话列表（含真实标题） | **直连该机 oc serve**：`GET {public_url}/session?directory={dir}`（Basic）→ `[{id, title, updatedAt, ...}]`                 | path-list.md 的 sections（仅 id，截断展示）  |

### override 的边界（重要）

`serv/web/{pcname}/config.md` 中配置的 `ip:port` **仅用作跳转地址（jump_url 生成），不改变数据面**：

- 数据面（探活 / 项目 / 会话查询）始终走 BFF → 设备公网域名（BFF 在服务器容器内，**无法访问**用户浏览器所在局域网的 `192.168.x.x`）；
- 跳转是浏览器**整页导航**（`<a href>` / 302），目标可以是任意公网或局域网地址，**无 CORS 约束** —— 这正是"便于本地测试"的实现基础；
- 效果：手机在外网时数据查询正常（公网域名），办公室电脑浏览器配置了局域网 override 后跳转直连内网（低延迟）。

---

## 7. 数据设计

### 7.1 devices.json（设备注册表，端侧上报）

云端路径：`serv/opencode/{sb_user}/devices.json`（与 path-list.md 同根命名空间，SB 私有空间）。

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
    },
    {
      "pctype": "windows",
      "pcname": "YG-PC",
      "public_url": "https://oc-win.isoops.com",
      "oc_serve_port": 9464,
      "reported_at": "2026-08-28T10:00:00+08:00",
      "version": "0.1.0"
    }
  ]
}
```

| 字段          | 来源                                               | 说明                              |
| ------------- | -------------------------------------------------- | --------------------------------- |
| `pctype`      | 端侧 `storage::paths::pctype()`                    | `macos` / `windows`               |
| `pcname`      | 端侧 `pcname()`（OS 用户名）                        | 与 path-list 命名空间同源          |
| `public_url`  | 端侧新增 env `OC_WEB_PUBLIC_URL`（TUI 设置面板配置）  | nginx 反代的该设备公网域名          |
| `reported_at` | 上报时间戳                                          | 仅供参考，在线状态以实时探活为准     |

**merge 规则**：端侧上报采用 GET → 按 `(pctype, pcname)` 键合并（本机条目覆盖，他机条目保留）→ PUT，union 语义兜底并发，最终一致即可。
**安全红线**：devices.json **不存任何密码**。

### 7.2 serv/web/{pcname}/config.md（门户用户配置，**按需求固定路径**）

云端路径：`serv/web/{pcname}/config.md`。`pcname` = **当前使用门户的浏览器所在电脑的系统名称**（本机标识）。

**pcname 的获取方式**：浏览器无法直接读取系统名 —— 门户设置页首次使用时要求输入「本机标识」（建议填系统用户名 / 主机名），保存在浏览器 localStorage；此后每次打开门户，前端将 localStorage 中的 pcname 提交给 BFF，BFF 按该 key 读写云端 config.md。换一台电脑打开门户即得到另一份独立配置（手机上通常不配置 override，走默认公网域名）。

```json
{
  "version": 1,
  "pcname": "samuel-mac",
  "updated_at": "2026-08-28T11:00:00+08:00",
  "jump_overrides": [
    { "device": "macos/samuel",  "base_url": "http://192.168.1.5:9464" },
    { "device": "windows/YG-PC", "base_url": "http://192.168.1.8:9464" }
  ],
  "default_base_url": "",
  "manual_devices": [
    { "id": "local-test", "label": "本机测试", "base_url": "http://127.0.0.1:9464" }
  ]
}
```

| 字段                | 作用                                                                                          |
| ------------------- | --------------------------------------------------------------------------------------------- |
| `jump_overrides[]`  | 按 `"pctype/pcname"` 覆盖该设备的跳转地址（ip:port 直连，便于本地/局域网测试）；数据面不受影响（§6） |
| `default_base_url`  | 全局兜底覆盖（留空 = 不覆盖；优先级低于 jump_overrides）                                        |
| `manual_devices[]`  | 手动添加的临时设备条目（如本机测试的 oc），仅在配置了 pcname 的浏览器中可见                       |

**跳转地址（base_url）解析优先级**：

```
jump_overrides[device]  >  default_base_url  >  devices.json 的 public_url
```

校验规则：base_url 必须是 `http://` 或 `https://` 开头的合法 URL（host 为 IP 或域名，可带端口），拒绝路径部分（仅保留 scheme://host:port）。

**格式说明**：文件扩展名 `.md` 但内容为 JSON（与现有 `path-list.md` 完全同风格，SB 不校验内容格式）。
**命名空间说明**：按需求固定为 `serv/web/{pcname}/...`（无 `{sb_user}` 段，SB 空间本身已是单用户 admin 私有）；若未来需要多 SB 用户隔离，可平滑升级为 `serv/web/{sb_user}/{pcname}/config.md`，读取端先试新版路径再回落旧版。

### 7.3 path-list.md（角色重定义）

- 现有 schema 与同步机制完全不动：`[{path, sections: [ses_xxx], createdAt, lastOpenedAt}]`；
- 角色从「TUI 项目/会话索引」扩展为「门户离线快照」；
- **Phase 2 可选升级**：sections 渐进迁移为 `[{id, title, updatedAt}]`（端侧同步时从 oc `/session` 合并元数据；读取端兼容字符串与对象两种形态）。

### 7.4 门户会话 Cookie（自身登录态）

- 服务端签名 Cookie（HMAC，密钥首次启动随机生成，持久化到容器 volume `/data`，0600）；
- HttpOnly + Secure + SameSite=Lax；有效期 7 天，滑动续期。

---

## 8. 端侧（mini-oc-gui-serve，本仓库）改造清单 —— 刻意最小化

| #   | 改动                                                                                                       | 位置                        | 量级 |
| --- | ---------------------------------------------------------------------------------------------------------- | --------------------------- | ---- |
| 1   | 新增设置项 `OC_WEB_PUBLIC_URL`（TUI 设置面板 + `PersistedSettings` + env key 常量 + `write_persisted_env`）   | `src/config.rs`、TUI 设置面板 | 小   |
| 2   | 启动时 + 每 5 分钟 PUT `devices.json`（SB 客户端 GET-merge-PUT；无 SB 配置时静默跳过）                          | `src/storage/` 新模块        | 小   |
| 3   | （可选，顺带）`/health` 返回 JSON `{opencode_running, rathole_running, version}`（保持 200 兼容现有探活语义）   | `src/handlers/health.rs`    | 极小 |
| 4   | （Phase 2）path-list sections 携带 `title/updatedAt`                                                            | `src/storage/sync.rs`       | 中   |

**不改**：supervisor、rathole 配置生成、Basic/Session 鉴权中间件、oc serve 启动参数。

---

## 9. 鉴权设计（三段链路）

```
浏览器 ──门户 Cookie──▶ mini-co-web ──统一 Basic──▶ 设备(端侧 API + oc serve)  [服务端持有]
浏览器 ──Basic 弹窗────▶ oc web（跳转后, 每域名/地址输一次, 浏览器缓存）       [用户手输]
```

1. **门户登录**：复用统一账密（`OPENCODE_SERVER_USERNAME/PASSWORD`）。部署时注入 mini-co-web 容器 env（§12）。前提约定：**各设备已在 TUI 设置面板配置为同一组账密**（现有 `PersistedSettings.username/password` 即做此用途）。
2. **门户 → 设备**：所有出站调用带 `Authorization: Basic <统一账密>`；账密仅存门户服务端内存，不落前端。
3. **浏览器 → oc web**：跳转后 Basic 弹窗，用户输入**同一组**账密。公网域名每域名缓存一次；局域网 override 地址（http://ip:port）同样弹一次。
4. **反模式（禁止）**：不在前端 localStorage / URL / devices.json / config.md 中传递设备账密（localStorage 仅存 pcname 标识）。

### 安全清单

| 项                       | 措施                                                                       |
| ------------------------ | -------------------------------------------------------------------------- |
| 门户登录爆破             | 按 IP 失败限速（5 次 / 10 分钟，内存计数）                                   |
| 凭据存储                 | 统一账密：各端 env 0600 + 门户容器 env（compose 文件 0600）；SB 不落密码      |
| 传输                     | 公网链路 HTTPS；override 的局域网 http 属用户自担的测试场景                   |
| 会话 Cookie              | HttpOnly / Secure / SameSite=Lax / HMAC 签名                                |
| 跳转目标校验             | jump 的 base_url 仅来自 devices.json 或 config.md 白名单（校验 scheme/host），拒绝任意 URL 重定向 |
| config.md 写入           | 仅登录态可写；pcname 白名单字符 `[A-Za-z0-9_-]{1,64}`                       |

---

## 10. 跳转设计

### 10.1 URL 生成规则（集中一处）

```
base_url = 解析优先级（§7.2）:
           config.md.jump_overrides[device] > config.md.default_base_url > devices.json.public_url

jump_url = {base_url}/{base64url_no_pad(directory)}/session/{session_id}
```

- 与 oc web 前端 `layout.tsx` 路由 `/​:dirBase64/session/:sessionId` 对齐；
- base64 使用 URL-safe 无填充变体（`-`/`_`，去除 `=`）；
- 生成逻辑收敛在 mini-co-web 的 `jump.rs` 单一模块，单元测试锁定（含中文路径 percent-encoding 场景）。

### 10.2 分层降级策略

```
用户点「进入会话」
   │
   ├─ 标准路径 (Phase 1): <a href={jump_url}> 整页导航
   │     ├─ 命中 sidebar → 直达会话 ✅
   │     └─ 未命中 → oc web 自动回落到会话列表页（用户手点一次, 可接受降级）
   │
   ├─ 增强路径 (Phase 2, "预热跳转"): 门户返回中间页, 页内嵌 1x1 隐藏 iframe
   │     加载 {base_url}/ 并注入 deep-link 事件:
   │     opencode://open-project?directory={dir}
   │     onload + 短延时后再 location.href = jump_url → 稳定直达
   │
   └─ 兜底路径: 导航到 {base_url}/ （oc web 首页, 手动选择项目会话）
```

**新会话创建流**：门户 `POST .../sessions` 创建 → 返回 `{directory, session_id, jump_url}` → 前端直接 `location.href = jump_url`。

### 10.3 本地测试场景（override 的典型用法）

```
办公室 Mac 浏览器:  设置页填本机标识 "samuel-mac"
                  → jump_overrides: macos/samuel = http://192.168.1.5:9464
                  → 会话跳转: http://192.168.1.5:9464/{b64}/session/{sid}  (局域网直达)
本地开发机:        manual_devices 添加 local-test = http://127.0.0.1:9464
                  → 设备卡片出现「本机测试」, 直接浏览/跳转本地 oc
手机 (外网):       未配置 override → 跳转走 https://oc-mac.isoops.com 公网域名
```

### 10.4 Phase 3（可选）：nginx 统一 SSO 消除 Basic 弹窗

- 门户与各 oc 域名同父域 `*.isoops.com`，门户登录后种父域 Cookie；
- 各 oc 域名的 nginx server 块加 `auth_request` 指向 mini-co-web 的校验端点；
- oc serve 侧不设 `OPENCODE_SERVER_PASSWORD`（关闭自身 Basic）；
- **安全前提**：rathole server 各服务端口（12333/13333）仅容器网络内可达（现状：`expose` 未映射宿主机，已满足）；改变安全模型，独立评审后再实施。

---

## 11. mini-co-web BFF 接口设计

所有 `/api/*` 需门户 Cookie（除 login）。错误统一 `{error: {code, message}}`。

| 方法 | 路径                                                                 | 说明                                                                                              |
| ---- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| POST | `/api/login` `{username, password}`                                    | 校验统一账密 → Set-Cookie；限速                                                                    |
| POST | `/api/logout`                                                          | 清除会话                                                                                            |
| GET  | `/api/devices`                                                          | `[{pctype, pcname, public_url, base_url(resolved), overridden, online, last_seen, version}]`；并行探活 |
| GET  | `/api/config?pcname={name}`                                             | 读云端 `serv/web/{pcname}/config.md`（无则返回默认空配置）                                            |
| PUT  | `/api/config?pcname={name}` `{config}`                                  | 校验后写回云端 config.md（GET-merge-PUT 防丢字段）                                                    |
| GET  | `/api/devices/{pctype}/{pcname}/projects`                               | `[{path, name, session_count, last_opened_at, stale}]`；在线走 `/project`，离线走云端快照             |
| GET  | `/api/devices/{pctype}/{pcname}/sessions?directory={dir}`               | `[{id, title, updated, from_cache}]`；在线直连 oc `/session`，离线走 path-list sections              |
| POST | `/api/devices/{pctype}/{pcname}/sessions` `{directory, title?}`        | 代理端侧 `POST /api/session` → `{id, directory, jump_url}`                                           |
| POST | `/api/manual/{device_id}/sessions`（manual 设备）                       | 同上，目标为 config.md 的 manual_devices 地址                                                         |
| GET  | `/api/devices/{pctype}/{pcname}/jump?directory={dir}&session={sid}`     | 校验目标 ∈ 该设备已知集合 → `302 Location` 深链（或 `?format=json` 返回 URL 供前端导航）              |
| GET  | `/healthz`                                                              | 门户存活（nginx / compose 健康检查）                                                                 |

**路径参数注意**：`directory` 为 query 参数（向 oc API 透传），需 percent-encoding；`pctype/pcname` 仅接受 `macos|windows` / `[A-Za-z0-9_-]+` 白名单字符。

### 前端页面流（SPA，移动优先）

```
/login                          登录页
/devices                        设备选择（win/macos 分组卡片 + manual 设备, 在线状态点, override 角标）
/devices/:pctype/:pcname/projects      项目列表（按 last_opened_at 排序, 快照角标）
/devices/:pctype/:pcname/sessions?dir= 会话列表（title + updated, 「继续」→ jump, 「新会话」→ POST 后跳转）
/settings                       设置页（本机标识 pcname / jump_overrides / manual_devices 管理）
/jump-preview                   （Phase 2）预热中间页：隐藏 iframe deep-link → 自动 location.href
```

H5 适配要点：列表项点击热区 ≥ 44px；跳转按钮用 `<a href>` 整页导航（不要 fetch + window.open，避免移动端弹窗拦截）；override 角标提示「跳转将直连局域网地址」。

---

## 12. 部署设计（服务器 Docker，重点章节）

### 12.1 docker-compose.yml 增量（加入现有 `8.159.159.138/docker-compose.yml`）

```yaml
  # ==========================================================================
  # 业务 3：mini-co-web - oc 会话门户（独立仓库项目）
  # 域名 web.isoops.com 反代到本容器
  # ==========================================================================
  mini-co-web:
    build:
      context: ../mini-co-web          # 方式A：服务器上 clone 独立仓库到同级目录源码构建
      dockerfile: Dockerfile
    # image: harbor.nnkcy.com/open-library/mini-co-web:0.1.0   # 方式B：推私有库后拉取
    container_name: mini-co-web
    restart: unless-stopped
    environment:
      - WEB_PORT=8100
      # 统一账密（与各设备 TUI 设置的 OPENCODE_SERVER_USERNAME/PASSWORD 一致）
      - OPENCODE_SERVER_USERNAME=opencode
      - OPENCODE_SERVER_PASSWORD=<统一账密>
      # SB 直连：走容器网络（app-net 内服务名互访, 低延迟零外网依赖）
      # cookie 名由 BFF 按 base_url 自动派生, 与 SB 按请求 host 派生的 Set-Cookie 天然一致
      - SB_BASE_URL=http://silverbullet:3000
      - SB_USER=admin
      - SB_PASSWORD=<SB密码>
      # 可选：改走公网 https://md.isoops.com（行为等价, 慢一跳）
      # - SB_BASE_URL=https://md.isoops.com
    volumes:
      - $PWD/mini-co-web/data:/data    # Cookie 签名密钥持久化
    networks:
      - app-net
    expose:
      - '8100'                          # 仅容器网络内可达, 不映射宿主机（经 nginx 访问）
    healthcheck:
      test: ["CMD", "wget", "-qO-", "http://127.0.0.1:8100/healthz"]
      interval: 30s
      timeout: 3s
      retries: 3
```

> 本地开发临时直连：在独立仓库自带的 compose 中加 `ports: ['8100:8100']`，访问 `http://<服务器或本机>:8100`；生产环境不映射。

### 12.2 Dockerfile（mini-co-web 仓库内）

```dockerfile
# ---------- 构建阶段 ----------
FROM rust:1.83-slim AS builder
WORKDIR /app
# 依赖层缓存
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
# 源码 + SPA 产物（web/dist 由 CI 或本地预先构建拷入）
COPY . .
RUN cargo build --release

# ---------- 运行阶段 ----------
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mini-co-web /usr/local/bin/mini-co-web
COPY --from=builder /app/web/dist /app/web
ENV WEB_STATIC_DIR=/app/web
WORKDIR /app
EXPOSE 8100
ENTRYPOINT ["/usr/local/bin/mini-co-web"]
```

> 说明：门户项目不受端侧 no-C-toolchain 约束，`reqwest` 直接用 `rustls-tls` feature（镜像无需 openssl）；SB 容器内直连为纯 http，亦无 TLS 依赖。

### 12.3 nginx 增量（`nginx/nginx.conf`）

```nginx
    # ===================== 新增 upstream =====================
    upstream mini_co_web {
        zone mini_co_web 64k;
        server mini-co-web:8100 resolve;          # 沿用现有 Docker DNS resolver 写法
        keepalive 16;
    }

    # ===================== 新增 server: web.isoops.com (80 → https) =====================
    server {
        listen       80;
        server_name  web.isoops.com;
        rewrite ^(.*)$ https://web.isoops.com;
        location = /nginx-health {
            access_log off;
            return 200 "ok\n";
            add_header Content-Type text/plain;
        }
    }

    # ===================== 新增 server: web.isoops.com (443) =====================
    server {
        listen       443 ssl;
        server_name  web.isoops.com;

        ssl_certificate "/etc/nginx/certs/ssl_isoops.pem";     # 复用现有证书
        ssl_certificate_key "/etc/nginx/certs/ssl_isoops.key";
        ssl_session_cache shared:SSL:1m;
        ssl_session_timeout  5m;
        ssl_ciphers ECDHE-RSA-AES128-GCM-SHA256:ECDHE:ECDH:AES:HIGH:!NULL:!aNULL:!MD5:!ADH:!RC4;
        ssl_protocols TLSv1.2 TLSv1.3;
        ssl_prefer_server_ciphers on;

        add_header X-Frame-Options "SAMEORIGIN" always;
        add_header X-Content-Type-Options "nosniff" always;
        add_header Referrer-Policy "no-referrer-when-downgrade" always;

        location = /nginx-health {
            access_log off;
            return 200 "ok\n";
            add_header Content-Type text/plain;
        }

        location / {
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
            # WebSocket（若 SPA 使用实时刷新）
            proxy_set_header Upgrade $http_upgrade;
            proxy_set_header Connection $connection_upgrade;
            proxy_pass http://mini_co_web;
        }
    }
```

> 若 `ssl_isoops` 为单域名证书而非泛域名，需为 `web.isoops.com` 补签证书并在 server 块中单独引用。

### 12.4 部署命令（服务器操作序列）

```bash
# 0) DNS：web.isoops.com → A 记录 8.159.159.138（域名商控制台操作, 一次性）

# 1) 拉取服务器配置仓库 + 独立项目仓库（首次）
cd /srv/8.159.159.138                       # 服务器上 compose 所在目录（按实际路径）
git clone <你的git托管地址>/mini-co-web.git ../mini-co-web   # 独立仓库落到同级目录

# 2) 配置 env（统一账密/SB 密码, 建议改用 env_file 并 chmod 600）
vim docker-compose.yml                       # 填 OPENCODE_SERVER_PASSWORD / SB_PASSWORD

# 3) 构建并启动（方式A: 源码构建）
docker compose build mini-co-web
docker compose up -d mini-co-web

#    或方式B: 私有库镜像
#   （本地）docker build -t harbor.nnkcy.com/open-library/mini-co-web:0.1.0 . && docker push ...
#   （服务器）docker compose pull mini-co-web && docker compose up -d mini-co-web

# 4) nginx 配置热更（先校验再 reload, 不中断现有服务）
docker compose exec nginx nginx -t
docker compose exec nginx nginx -s reload

# 5) 验证
curl -I https://web.isoops.com/healthz       # 期望 200
docker compose logs -f mini-co-web           # 观察启动日志

# 6) 后续升级
cd ../mini-co-web && git pull
cd - && docker compose build mini-co-web && docker compose up -d mini-co-web
```

### 12.5 网络与连通性核对清单

| 链路                                        | 通路                                                                                |
| ------------------------------------------- | ----------------------------------------------------------------------------------- |
| 浏览器 → 门户                               `web.isoops.com:443` → 宿主机 → nginx 容器 → `mini-co-web:8100`（app-net）              |
| mini-co-web → SB                            容器内 → `silverbullet:3000`（app-net 直达，无需公网出口）                                |
| mini-co-web → 设备公网域名                   容器出站 → 公网 DNS → `oc-mac.isoops.com`（回程经 nginx → rathole 12333 → 端侧）         |
| 浏览器 → oc web（跳转）                      整页导航直达 `oc-*.isoops.com` 或 override 的局域网地址                                   |
| rathole 服务端口暴露面                        12333/13333 仅 app-net 内（expose 未映射宿主机）→ 公网不可直连，满足 §10.4 SSO 前提       |

---

## 13. 关键流程时序（正常路径）

```
浏览器            mini-co-web        SB(md/silverbullet)    设备(oc-mac域名→rathole→端侧)
  │ login ───────────▶│                       │                     │
  │◀── Set-Cookie ────│                       │                     │
  │ GET /api/config?pcname=samuel-mac ─────────▶                     │
  │◀── jump_overrides (含局域网 override)       │                     │
  │ GET /api/devices ─▶│ ── GET devices.json ──▶                     │
  │                    │ ── GET /health ──────────────────────────▶ │
  │◀── 设备卡片(在线+override角标)              │                     │
  │ GET projects ──────▶│ ── Basic GET /project ──────────────────▶ │
  │ GET sessions ──────▶│ ── Basic GET oc:/session?dir ───────────▶ │
  │◀── 会话列表(真实title)                      │                     │
  │ 点「进入」→ <a href=jump_url>               │                     │
  │ ────────── 整页导航 http://192.168.1.5:9464/{b64}/session/{sid} ─▶│(局域网直达)
  │   (Basic 弹窗输统一账密一次)                 │                     │
  │◀───────────── oc web 打开对应会话 ──────────────────────────────│
```

---

## 14. 错误处理与降级矩阵

| 场景                            | 行为                                                                                     |
| ------------------------------- | ---------------------------------------------------------------------------------------- |
| 设备探活超时 / 非 200            | 标记离线；项目/会话列表自动切云端快照，响应 `from_cache` 系列                                |
| 设备在线但 `/project` 401        | 响应 502 + `device_auth_failed`（统一账密与该设备不一致时提示用户到 TUI 核对设置）            |
| oc `/session` 单项目失败         | 该项目会话列表降级为快照；其余项目不受影响（按目录粒度隔离错误）                              |
| SB 不可达                       | 设备列表与 config.md 使用进程内上次缓存（stale 标注）；无缓存则提示无法列举设备                |
| override 地址不可达（设备关机等） | 跳转由浏览器原生报错；前端在 override 角标 tooltip 提示「直连地址失败时请检查设备在线」        |
| jump 目标不存在于已知集合        | 400 `invalid_target`（防开放重定向）                                                       |
| config.md 的 pcname 非法        | 400 `invalid_pcname`（白名单 `[A-Za-z0-9_-]{1,64}`）                                       |
| 深链被 oc web 回落列表页         | 属预期降级（§10.2），Phase 2 预热消除                                                      |
| 门户 Cookie 过期                 | 401 → 前端跳登录页，登录后回跳原路由                                                         |

---

## 15. 测试策略

| 层                  | 用例                                                                                              |
| ------------------- | ------------------------------------------------------------------------------------------------- |
| mini-co-web 单测    | jump.rs 深链生成（base64url / 中文路径 / override 优先级）；base_url 校验（拒绝路径与非法 scheme）；pcname 白名单 |
| SB 客户端集成       | login → cookie 派生（http://silverbullet:3000 与 https://md.isoops.com 两种 base_url）；config.md GET-merge-PUT |
| BFF 集成            | mock 设备（wiremock）：在线全链路 / 401 / 超时降级三级；devices.json 与 config.md 合成设备列表                  |
| 部署验收            | compose up 后 `curl /healthz`；nginx reload 后 `curl -I https://web.isoops.com`；容器重启后 Docker DNS 重解析     |
| 端到端验收          | 手机浏览器：登录 → 选 mac → 选项目 → 选会话 → 直达 oc web（≤1 次 Basic 弹窗 + 最多一次手动选会话）              |
| override 场景       | 局域网浏览器配置 override 后跳转直连 ip:port；手机无 override 走公网域名；两者互不影响                           |
| 兼容性              | iOS Safari / Android Chrome 的 Basic 弹窗与缓存行为抽样                                            |

---

## 16. 风险与对策

| 风险                                         | 等级        | 对策                                                                                              |
| -------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------- |
| oc web 深链 sidebar 限制导致回落列表页        | 高（体验）   | 分层降级（§10.2）+ Phase 2 iframe deep-link 预热；jump.rs 集中生成 URL，oc 升级变更时一处适配        |
| oc 版本升级改变路由格式（base64 路径）        | 中          | 同上集中生成；可探测 oc 版本并记录                                                                  |
| Basic 弹窗在移动端体验参差（保存行为不一）     | 中          | MVP 接受；Phase 3 nginx `auth_request` 统一 SSO（§10.4）                                             |
| **双仓库 schema 漂移**（devices.json / config.md 契约） | 中   | 契约以本文档 §7 为规范；两端各自实现解析时以本文档为准；schema 变更须先改文档再同步两端仓库，CI 加契约快照测试 |
| devices.json 并发覆盖                        | 低          | union merge 语义，最终一致（§7.1）                                                                   |
| 统一账密单点泄露即全域失守                    | 中          | §9 安全清单：0600 存储、HTTPS、限速、不落前端/云端                                                    |
| override 仅影响跳转、数据面仍走公网           | 低（预期行为）| 文档明确（§6）；未来若要数据面也直连，需浏览器直连设备 API（oc `--cors` 白名单门户域名）——列为后续演进 |
| iframe 预热被 oc web X-Frame-Options 拦截     | 低          | Phase 2 实测；被拦则停留在「回落列表页」降级路径，功能不损                                            |
| `ssl_isoops` 证书不覆盖 web.isoops.com        | 低          | 部署前核对证书 SAN；必要时补签泛域名证书                                                              |

---

## 17. 实施阶段划分

| 阶段               | 内容                                                                                                                            | 交付判据                                                                                              |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Phase 1（MVP）** | 端侧改造 #1 #2 #3（mini-oc-gui 仓库）；mini-co-web 独立仓库（login / devices / projects / sessions / jump + 探活 + 快照兜底 + config.md override / 设置页）；服务器 compose + nginx + DNS | 手机浏览器完成「选 mac → 选项目 → 选会话 → 直达 oc web（≤1 次 Basic + 最多一次手动选会话）」；局域网浏览器 override 直连跳转可用 |
| **Phase 2（体验）** | iframe deep-link 预热跳转；path-list sections 元数据升级（离线真实标题）；`/health` 状态字段消费                                    | 全新浏览器首跳即直达会话；设备离线时会话列表仍显示真实标题                                              |
| **Phase 3（可选）** | nginx `auth_request` 全域 SSO，消除 Basic 弹窗                                                                                    | 全程零弹窗跳转；安全前提复核通过（rathole 端口仅容器网络可达 —— 现状已满足）                             |

---

## 18. 设计决策记录（ADR 摘要）

| #  | 决策                                                                                                     | 理由                                                                                                   |
| - | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| 1  | BFF 模式（服务端代理设备调用），而非纯静态 SPA 浏览器直连                                                 | 规避 oc serve CORS 白名单（§3.2-1）；统一账密不出服务端                                                   |
| 2  | 会话真数据源 = oc serve 原生 `/session` API；path-list.md 降级为离线快照                                    | 修正现有伪造 title 缺陷；离线仍可浏览                                                                     |
| 3  | 设备目录 = 云端 devices.json 显式注册（含 public_url）                                                      | 确定性高；端侧自助上报；SB 无目录列举依赖                                                                 |
| 4  | 统一账密复用 `OPENCODE_SERVER_USERNAME/PASSWORD`（各设备同值）                                              | 现状已是「一套账密双生效」，门户顺延该模型，用户零新增记忆                                                  |
| 5  | 跳转 URL 由 mini-co-web 的 jump.rs 集中生成；base_url 解析优先级 override > default > devices.json          | 隔离 oc web 路由格式变化；满足本地测试诉求                                                                  |
| 6  | **override（ip:port）仅影响跳转地址，不影响数据面**                                                        | BFF 在服务器容器内不可达用户局域网；跳转为浏览器整页导航无 CORS 限制 —— 用户需求的「跳转地址」语义即此       |
| 7  | 门户配置存云端 `serv/web/{pcname}/config.md`（pcname = 浏览器所在电脑系统名，localStorage + 设置页输入）      | 按需求固定路径；多电脑各有独立跳转偏好；手机默认不受局域网 override 影响                                    |
| 8  | mini-co-web 独立仓库 + Docker 部署（app-net，expose 8100，仅经 nginx 访问）                                 | 与服务器现有部署形态一致；SB 走容器网络直连；契约以本文档为规范双仓同步                                     |
| 9  | devices.json / config.md 不存密码；前端不持设备凭据                                                        | 安全红线（§9）                                                                                            |

---

## 19. 开放问题（实施前确认）

1. **项目名拼写**：按指示使用 `mini-co-web`（若实际期望为 `mini-oc-web`，全局替换即可，不影响任何设计逻辑）。
2. **统一账密落地**：各设备 TUI 设置面板将 `OPENCODE_SERVER_USERNAME/PASSWORD` 配置为同值（一次性人工操作，部署前提）。
3. **域名**：示例采用 `web.isoops.com`；确认可用性及 `ssl_isoops` 证书是否覆盖（否则补签）。
4. **config.md 命名空间**：按需求固定 `serv/web/{pcname}/config.md`（无 sb_user 段）；若未来多 SB 用户需隔离，升级路径见 §7.2。
5. **oc web 路由稳定性**：`/{base64(dir)}/session/{id}` 为当前版本行为，需在目标 oc 版本实测；若不支持，Phase 1 退化为「跳首页 + 手选」，深链作为增强。
6. **iframe 预热的 X-Frame-Options 行为**：Phase 2 实测（§16）。
