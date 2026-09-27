export interface ApiError {
  code: string
  message: string
}

export class UnauthorizedError extends Error {
  constructor(message = 'unauthorized') {
    super(message)
    this.name = 'UnauthorizedError'
  }
}

/** 带 BFF 错误码的 API 错误（对应后端 AppError 序列化的 error.code）。 */
export class ApiError extends Error {
  code: string
  constructor(code: string, message: string) {
    super(message)
    this.name = 'ApiError'
    this.code = code
  }
}

async function handle<T>(r: Response): Promise<T> {
  if (r.status === 401) {
    // 重要：不要在这里 window.location.href = '/login'，否则 store
    // 初始化时的探测会在每次未登录访问（包括 /login 本身）触发整页
    // 跳转，形成死循环。改为抛错，由调用方（路由守卫 / 组件）决定
    // 用 router.push 软跳转到 /login。
    throw new UnauthorizedError()
  }
  if (!r.ok) {
    const body = await r.json().catch(() => null)
    const msg = body?.error?.message ?? `${r.status} ${r.statusText}`
    // 设备级失败（如 device_auth_failed）靠 code 区分，页面据此弹授权框。
    if (body?.error?.code) {
      throw new ApiError(body.error.code, msg)
    }
    throw new Error(msg)
  }
  return r.json()
}

// no-store：BFF 的 GET 响应无 Cache-Control 头，浏览器启发式缓存会让
// /jump 等含凭据的响应陈旧化（失效 token → opencode 弹 Basic Auth 框）。
export const apiGet = <T>(path: string, init?: RequestInit): Promise<T> =>
  fetch(path, { credentials: 'include', cache: 'no-store', ...init }).then((r) => handle<T>(r))

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

export const apiDelete = <T>(path: string): Promise<T> =>
  fetch(path, { method: 'DELETE', credentials: 'include' }).then((r) => handle<T>(r))

// --- typed domain objects matching BFF routes.rs ---

export interface ProjectInfo {
  path: string
  lastOpenedAt?: string
}

export interface SessionInfo {
  id: string
  title?: string
  updatedAt?: string
}

export interface CreatedSession {
  id: string
  directory?: string
  jump_url: string
}

/** 设备清单条目：描述 + 服务名称 + 端口 + 设备名称（客户端绑定写入）+ 平台类型 + 绑定状态 + 设备 URL（admin 录入）。 */
export interface UserDevice {
  /** 描述（人类可读，展示用；管理表单必填 1-64 字符；旧数据 d-name 已由服务端迁移为 desc）。 */
  desc: string
  /** 设备服务名称（路径/URL 材料）。 */
  name: string
  port: number
  /** OpenCode 端口号：TUI 启动 oc server 的端口（默认 9464；跳转 oc web 用）。 */
  'oc-port'?: number
  /** 设备名称 —— 仅由绑定客户端（POST /api/device-bind）写入；管理表单只读展示，空 = 尚未绑定。 */
  'device-name': string
  /** 平台类型：'windows' | 'macos'（与 SB 路径列表平台类型同枚举）。 */
  pctype: string
  /** 绑定状态（默认 false）。 */
  bound?: boolean
  /**
   * 设备可达 URL，admin 在分配时录入（HTTP/HTTPS）。后端用其探测
   * `<public-url>/status`，SPA 用其构建 deep-link。为空（旧数据）时
   * 后端不探测，报 online=false + reason"未配置"。
   */
  'public-url'?: string
  // /api/me 返运行时状态；其他接口不返，字段为可选
  /**
   * 是否在线：设备 /status 接口能正常返回且应答平台与条目 pctype
   * 一致时为 true。多个条目可能解析到同一探测 URL，此时其他设备
   * 的应答会被判为不在线（reason 说明平台不匹配），避免一台设备
   * 启动后所有绑定条目都显示可用。
   */
  online?: boolean
  /** opencode 是否在线：设备 /status 接口返回的 opencode_serve 字段为 "running"。 */
  opencode_online?: boolean
  /**
   * 可用状态：-1 / 0 / 1，由后端根据 (bound, online, opencode_online) 计算：
   *  1 = 完全可用（bound && online && opencode_online）
   *  0 = 已连接但未完全可用（online=true 但 bound 或 opencode_online 缺失）
   * -1 = 不可用（/status 探测失败）
   * 前端按 -1/0/1 → 红/橙/绿 显示。
   */
  available?: number
  /** 探测失败原因（online=false 时填）。 */
  reason?: string
  /** 设备端 /status 响应体（online=true 时填）。 */
  status?: DeviceStatusInfo
}

/** 用户级 SB 连接配置：域名（默认 https://sb.example.com）/账号/密码。 */
export interface SbConfig {
  base_url: string
  username: string
  password: string
}

/** GET /api/me — 当前登录用户（多租户），不含密钥。 */
export interface MeResponse {
  id: string
  name: string
  last_used_at?: string
  devices: UserDevice[]
  sb: SbConfig
}

/** mini-oc-gui `GET /status` 的设备快照（由 BFF 代理透传）。 */
export interface DeviceStatusInfo {
  opencode_serve?: 'running' | 'stopped'
  rathole?: 'running' | 'stopped'
  started_at?: string | null
  system?: { os?: string; arch?: string; hostname?: string }
  version?: string
}

/** GET /api/device-status/:port — 设备实际状态探测结果。 */
export interface DeviceStatus {
  online: boolean
  reason?: string
  status?: DeviceStatusInfo
}

/**
 * GET /api/devices/:pctype/:pcname/detail — 设备连接详情，含设备端
 * opencode serve 的 Basic 凭据。SessionsPage 预取它为降级跳转链接拼
 * `?auth_token=`（编码对齐 BFF jump.rs），避免触发浏览器原生弹窗。
 */
export interface DeviceDetail {
  pctype: string
  pcname: string
  public_url: string
  port?: number
  oc_port?: number
  username: string
  password: string
}

/** 管理员接口返回的租户用户（含完整 key，仅 /admin 使用）。 */
export interface PortalUser {
  id: string
  name: string
  key: string
  devices: UserDevice[]
  sb?: SbConfig
  created_at?: string
  updated_at?: string
}

/** 创建/更新用户请求体（sb 为管理员分配的 SB 存储配置，可选）。 */
export interface UserBody {
  name: string
  devices: UserDevice[]
  sb?: SbConfig
}

/** GET /api/admin/info — 服务基础信息（含管理员可见的敏感密钥）。 */
export interface AdminInfo {
  version: string
  web_bind: string
  web_port: number
  portal_base: string
  sb_base_url: string
  sb_user: string
  sb_password: string
  hostname: string
  local_ips: string[]
  uptime_secs: number
  user_count: number
}