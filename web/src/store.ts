import { defineStore } from 'pinia'
import { ref } from 'vue'
import { apiGet, apiPost, apiPut, apiDelete, UnauthorizedError } from './api'
import type { AdminInfo, MeResponse, PortalUser, UserBody } from './api'

export const useAuthStore = defineStore('auth', () => {
  const me = ref<MeResponse | null>(null)
  const loading = ref(false)

  /**
   * Probe whether the current request has a valid session cookie.
   * Returns `true` if logged in, `false` if 401.
   *
   * IMPORTANT: never call this from the store's setup IIFE — that runs
   * on every page mount (including `/login`), and an old version did
   * `window.location.href = '/login'` on 401 which caused infinite reload.
   * Call from the router beforeEach guard instead, where the result can
   * route to `/login` via `router.push()` (no full page reload).
   */
  async function probe(): Promise<boolean> {
    try {
      me.value = await apiGet<MeResponse>('/api/me')
      return true
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        me.value = null
      }
      // Network / 5xx: keep previous me (don't flip to logged-out on
      // transient errors), but signal not-logged-in for routing.
      return false
    }
  }

  /** 密钥登录：仅 32 位 key，无账号。 */
  async function login(key: string) {
    loading.value = true
    try {
      await apiPost('/api/login', { key })
      await probe()
    } finally {
      loading.value = false
    }
  }

  async function logout() {
    try {
      await apiPost('/api/logout', {})
    } catch {
      // ignore
    }
    me.value = null
  }

  return { me, loading, probe, login, logout }
})

/** 超级管理员（SB 密码登录）状态。 */
export const useAdminStore = defineStore('admin', () => {
  const info = ref<AdminInfo | null>(null)
  const users = ref<PortalUser[]>([])
  const loading = ref(false)

  /** 探测管理员会话是否有效（同时缓存服务信息）。 */
  async function probe(): Promise<boolean> {
    try {
      info.value = await apiGet<AdminInfo>('/api/admin/info')
      return true
    } catch {
      return false
    }
  }

  async function login(password: string) {
    loading.value = true
    try {
      await apiPost('/api/admin/login', { password })
    } finally {
      loading.value = false
    }
  }

  async function logout() {
    try {
      await apiPost('/api/admin/logout', {})
    } catch {
      // ignore
    }
    info.value = null
    users.value = []
  }

  async function loadUsers() {
    users.value = await apiGet<PortalUser[]>('/api/admin/users')
  }

  async function createUser(body: UserBody): Promise<PortalUser> {
    return apiPost<PortalUser>('/api/admin/users', body)
  }

  async function updateUser(id: string, body: UserBody): Promise<PortalUser> {
    return apiPut<PortalUser>(`/api/admin/users/${encodeURIComponent(id)}`, body)
  }

  async function regenerateKey(id: string): Promise<string> {
    const r = await apiPost<{ key: string }>(
      `/api/admin/users/${encodeURIComponent(id)}/regenerate-key`,
      {},
    )
    return r.key
  }

  async function deleteUser(id: string): Promise<void> {
    await apiDelete(`/api/admin/users/${encodeURIComponent(id)}`)
  }

  return {
    info,
    users,
    loading,
    probe,
    login,
    logout,
    loadUsers,
    createUser,
    updateUser,
    regenerateKey,
    deleteUser,
  }
})
