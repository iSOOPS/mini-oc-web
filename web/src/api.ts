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

/** BFF 会话有效，但设备端拒绝了 BFF 持有的凭据（code=device_auth_failed）。 */
export const DEVICE_AUTH_FAILED = 'device_auth_failed'

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

export const apiGet = <T>(path: string, init?: RequestInit): Promise<T> =>
  fetch(path, { credentials: 'include', ...init }).then((r) => handle<T>(r))

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

export interface DeviceView {
  pctype: 'macos' | 'windows'
  pcname: string
  pcname_b64: string
  portal_base: string
  public_url: string
  online: boolean
  version: string | null
}

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

/** 用户级 SB 连接配置：域名（默认 https://md.isoops.com）/账号/密码。 */
export interface SbConfig {
  base_url: string
  username: string
  password: string
}

/** GET /api/me — 当前登录用户（多租户），不含密钥。 */
export interface MeResponse {
  id: string
  name: string
  cloud_ip: string
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
  rathole_key: string
  hostname: string
  local_ips: string[]
  uptime_secs: number
  user_count: number
  device_count: number
}