import { ref } from 'vue'
import { apiPost, UnauthorizedError } from './api'

/**
 * 设备授权弹框的共享状态与动作。
 *
 * 页面在两种时机打开它：
 * 1. DevicesPage 点击设备卡片，`GET .../auth-check` 返回 `{required:true}`；
 * 2. Projects/Sessions 页面加载遇 `device_auth_failed`（如直接 URL 访问、
 *    或 BFF 重启后凭据缓存丢失）。
 *
 * 授权成功（BFF 验证并缓存凭据）后回调 `onSuccess` —— 由调用方决定
 * 跳转还是重新加载。
 */
export function useDeviceAuth() {
  const visible = ref(false)
  const busy = ref(false)
  const error = ref('')
  const baseUrl = ref('')

  let target: { pctype: string; pcname: string; done: () => void } | null = null

  function open(pctype: string, pcname: string, url: string, done: () => void) {
    target = { pctype, pcname, done }
    baseUrl.value = url
    error.value = ''
    visible.value = true
  }

  async function submit(username: string, password: string) {
    if (!target) return
    const { pctype, pcname, done } = target
    busy.value = true
    error.value = ''
    try {
      await apiPost(`/api/devices/${pctype}/${encodeURIComponent(pcname)}/auth`, {
        username,
        password,
      })
      visible.value = false
      target = null
      done()
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        // BFF 会话失效：关弹框交给路由守卫处理
        visible.value = false
        target = null
        return
      }
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      busy.value = false
    }
  }

  function cancel() {
    if (busy.value) return
    visible.value = false
    target = null
  }

  return { visible, busy, error, baseUrl, open, submit, cancel }
}
