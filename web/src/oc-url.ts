import type { UserDevice } from './api'

/**
 * opencode 会话直达 URL（格式来自 2026-09-24 免登录验证报告）：
 *
 *   {scheme}://{user}:{pass}@{host}:{oc_port}/server/{base64url(serverURL)}/session/{sid}
 *
 * - opencode serve 鉴权是整站 HTTP Basic Auth，官方无 query/cookie 认证
 *   旁路；URL 内嵌凭据是浏览器自动转 `Authorization` 头的唯一免弹窗
 *   通道（Chromium 清缓存 + 全新隔离环境实测直达）。
 * - `/server/{base64url(serverURL)}/session/{sid}` 是 Web UI 的规范会话
 *   路由（逆向 bundle 确认）；`serverURL` 是含 scheme 的完整
 *   `scheme://host:port` 串。
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
  const authority = `${schemeIdx === -1 ? base : base.slice(schemeIdx + 3)}:${device['oc-port'] ?? 9464}`
  const serverUrl = `${scheme}${authority}`
  const b64 = btoa(serverUrl)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  const userinfo = `${encodeURIComponent(cred.username)}:${encodeURIComponent(cred.password)}`
  return `${scheme}${userinfo}@${authority}/server/${b64}/session/${sessionId}`
}
