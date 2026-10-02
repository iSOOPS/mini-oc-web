import type { UserDevice } from './api'

/**
 * opencode 会话直达 URL（格式来自 2026-09-24 免登录验证报告）：
 *
 *   {scheme}://{user}:{pass}@{host}[:{port}][/{oc-port}]/server/{base64url(serverURL)}/session/{sid}
 *
 * - opencode serve 鉴权是整站 HTTP Basic Auth，官方无 query/cookie 认证
 *   旁路；URL 内嵌凭据是浏览器自动转 `Authorization` 头的唯一免弹窗
 *   通道（Chromium 清缓存 + 全新隔离环境实测直达）。
 * - `/server/{base64url(serverURL)}/session/{sid}` 是 Web UI 的规范会话
 *   路由（逆向 bundle 确认）；`serverURL` 是浏览器实际访问的完整地址
 *   （含穿透地址自带端口与 oc-port 路径前缀）。
 * - 穿透地址（public-url）可自带端口（`ip:port` 隧道端点）；OpenCode
 *   端口号非空时以路径前缀 `/{oc-port}` 拼接（反代路径分流），为空时
 *   跳转地址即穿透地址本身、不含额外段 —— 与 BFF `oc_base_url` 契约一致。
 * - 凭据经 userinfo 传输（悬停/历史记录可见明文），与报告方案 1 的
 *   安全权衡一致，仅限受信任的门户用户使用。
 */
export function buildOcSessionUrl(
  device: Pick<UserDevice, 'public-url' | 'oc-port'>,
  cred: { username: string; password: string },
  sessionId: string,
): string {
  const base = (device['public-url'] ?? '').replace(/\/+$/, '')
  if (!base || (!cred.username && !cred.password)) return ''
  const schemeIdx = base.indexOf('://')
  const scheme = schemeIdx === -1 ? 'http://' : `${base.slice(0, schemeIdx + 3)}`
  const rest = schemeIdx === -1 ? base : base.slice(schemeIdx + 3)
  const ocPort = device['oc-port']
  // authority = host[:port]（第一个 / 之前）；prefix = /{oc-port} 路径前缀
  const slash = rest.indexOf('/')
  const authority = slash === -1 ? rest : rest.slice(0, slash)
  const prefix = ocPort != null ? `/${ocPort}` : ''
  const serverUrl = `${scheme}${authority}${prefix}`
  const b64 = btoa(serverUrl)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  const userinfo = `${encodeURIComponent(cred.username)}:${encodeURIComponent(cred.password)}`
  return `${scheme}${userinfo}@${authority}${prefix}/server/${b64}/session/${sessionId}`
}
