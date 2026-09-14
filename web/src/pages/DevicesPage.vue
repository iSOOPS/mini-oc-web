<template>
  <div>
    <div class="page-head">
      <h1>选择设备</h1>
      <button
        class="ghost refresh-btn"
        :disabled="refreshing !== null"
        @click="refreshAll"
      >
        {{ refreshing !== null ? '刷新中…' : '刷新状态' }}
      </button>
    </div>

    <p v-if="error" class="error">{{ error }}</p>

    <div v-if="devices.length === 0" class="card empty">
      <p>还没有可用的设备。</p>
      <p class="muted">
        设备清单由管理员在管理后台授权分配;如需添加设备，请联系管理员。
      </p>
    </div>

    <div v-else class="stack">
      <div
        v-for="d in devices"
        :key="d.name + ':' + d.port"
        class="device-card"
        :class="{ 'card-disabled': !isOnline(d) }"
      >
        <a href="#" class="device-main" @click.prevent="onDeviceClick(d)">
          <!-- 行1 头部：状态点 + 设备名（可换行）+ 绑定徽章右对齐 -->
          <div class="card-head">
            <span class="dot" :class="dotClass(d)"></span>
            <strong class="name">{{ displayName(d) }}</strong>
            <span class="chip bound" :class="d.bound ? 'bound-on' : 'bound-off'">
              {{ d.bound ? '已绑定' : '未绑定' }}
            </span>
          </div>
          <!-- 行2 元信息徽章：容器可换行，服务名超长时徽章内部折行 -->
          <div class="card-meta">
            <span class="chip service">服务名 {{ d.name }}</span>
            <span v-if="d.pctype" class="chip platform">{{ d.pctype }}</span>
            <span class="chip port">端口 {{ d.port }}</span>
          </div>
          <!-- 行3 状态行 -->
          <div class="card-status muted small">
            <div v-if="statusOf(d)">
              <span :class="ocServe(d) === 'running' ? 'ok-text' : 'warn-text'">
                oc serve {{ ocServe(d) }}
              </span>
              <template v-if="hostOf(d)"> · {{ hostOf(d) }}</template>
            </div>
            <div v-else>状态未知</div>
          </div>
          <!-- 行4 云隧道地址 -->
          <div class="path">{{ cloudBase }}:{{ d.port }}</div>
        </a>
      </div>
    </div>

    <LoadingOverlay
      :visible="refreshing !== null"
      :progress="refreshing ?? undefined"
      title="正在刷新设备状态…"
      cancelable
      @cancel="cancelRefresh"
    />

    <!-- 点击拦截 Toast：单例浮层，最新消息覆盖旧消息；点击关闭，3.5s 自动消失 -->
    <Transition name="toast">
      <div v-if="toast" class="toast" :class="'tone-' + toastTone" @click="dismissToast">
        {{ toast }}
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { apiGet, UnauthorizedError } from '../api'
import type { DeviceStatus, UserDevice } from '../api'
import { useAuthStore } from '../store'
import LoadingOverlay from '../components/LoadingOverlay.vue'

const auth = useAuthStore()
const router = useRouter()

const error = ref('')

// 全量刷新状态：null = 空闲；非 null = 正在刷新（含菊花进度）
const refreshing = ref<{ done: number; total: number } | null>(null)
let refreshAbort: AbortController | null = null

async function refreshAll() {
  if (refreshing.value !== null) return         // 防重入
  const ctrl = new AbortController()
  refreshAbort = ctrl
  const initialDevices = auth.me?.devices ?? []
  refreshing.value = { done: 0, total: initialDevices.length }

  try {
    // (a) 重拉 /api/me
    // probe() 不抛 UnauthorizedError/network 错误——只抛 AbortError 并返回 false。
    // 401 与网络错误的区分靠 me 是否被清空（probe 内部约定）。
    const ok = await auth.probe(ctrl.signal).catch(() => false)
    if (ctrl.signal.aborted) return             // 阶段 (a) 被取消
    if (!ok) {
      if (auth.me === null) {                   // 401：probe 已清空 me
        refreshing.value = null
        refreshAbort = null
        router.push('/login')                   // 路由守卫会接管后续；这里显式触发
        return
      }
      // 网络/5xx：probe 保留旧 me（按 store.ts 的「不轻易登出」约定）
      refreshing.value = null
      refreshAbort = null
      showToast('刷新设备清单失败，请稍后重试', 'warn')
      return
    }

    // (b) 并发刷 device-status（批 ≤ 6）
    const devices = auth.me?.devices ?? []
    const BATCH = 6
    for (let i = 0; i < devices.length; i += BATCH) {
      if (ctrl.signal.aborted) break
      const batch = devices.slice(i, i + BATCH)
      await Promise.allSettled(
        batch.map((d) =>
          apiGet<DeviceStatus>(`/api/device-status/${d.port}`, { signal: ctrl.signal })
            .then((r) => {
              // Race guard: fetch 可能在 abort 后才 resolve（极少数情况），
              // 此时已不算「本次刷新」的结果，不写入 statuses。
              if (ctrl.signal.aborted) return
              statuses.value = { ...statuses.value, [d.port]: r }
            })
            .catch((e) => {
              // 批次中若出现 401，由页面级路由守卫在下次导航时兜底跳 /login。
              if (ctrl.signal.aborted) return
              statuses.value = {
                ...statuses.value,
                [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
              }
            })
            .finally(() => {
              if (refreshing.value) {
                refreshing.value = { ...refreshing.value, done: refreshing.value.done + 1 }
              }
            }),
        ),
      )
    }
  } finally {
    const wasCancelled = ctrl.signal.aborted
    refreshing.value = null
    refreshAbort = null
    if (wasCancelled) showToast('已取消刷新', 'muted')
  }
}

function cancelRefresh() {
  refreshAbort?.abort()
}

async function silentFirstLoad() {
  // 1) 拿到最新设备清单（无 signal、不弹菊花）。401 由路由守卫处理。
  await auth.probe().catch(() => {})
  const devices = auth.me?.devices ?? []
  // 2) fire-and-forget 触发每台设备状态探测。不 await、不写 refreshing。
  for (const d of devices) {
    apiGet<DeviceStatus>(`/api/device-status/${d.port}`)
      .then((r) => {
        statuses.value = { ...statuses.value, [d.port]: r }
      })
      .catch((e) => {
        if (e instanceof UnauthorizedError) return
        statuses.value = {
          ...statuses.value,
          [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
        }
      })
  }
}

// 用户设备清单（管理员授权）直接驱动卡片;状态通过云隧道主动探测。
const devices = computed(() => auth.me?.devices ?? [])
const cloudBase = computed(() => auth.me?.cloud_ip ?? '')

/** 卡片主标题：设备名称（客户端绑定写入）→ 描述 → 服务名。 */
function displayName(d: UserDevice): string {
  return d['device-name'] || d.desc || d.name
}

// port → 探测结果
const statuses = ref<Record<number, DeviceStatus>>({})

function statusOf(d: UserDevice): DeviceStatus | undefined {
  return statuses.value[d.port]
}
function isOnline(d: UserDevice): boolean {
  return statusOf(d)?.online === true
}
function ocServe(d: UserDevice): string {
  return statusOf(d)?.status?.opencode_serve ?? 'unknown'
}
function rathole(d: UserDevice): string {
  return statusOf(d)?.status?.rathole ?? 'unknown'
}
function hostOf(d: UserDevice): string {
  return statusOf(d)?.status?.system?.hostname ?? ''
}

/**
 * 状态点颜色（按优先级）：
 * 红 = 未连接上（探测失败 / rathole 未运行）
 * 黄 = 已连上但不可用（未绑定 / oc serve 未运行）
 * 绿 = 完全可用；灰 = 探测中 / 状态未知（数据未到不闪红黄）
 */
function dotClass(d: UserDevice): string {
  const st = statusOf(d)
  if (!st) return 'offline'
  if (!st.online || rathole(d) !== 'running') return 'danger'
  if (!d.bound || ocServe(d) !== 'running') return 'warn'
  return 'online'
}

// —— 点击拦截 Toast（页面级单例：新消息替换旧消息并重置计时）——
type ToastTone = 'danger' | 'warn' | 'muted'
const toast = ref('')
const toastTone = ref<ToastTone>('muted')
let toastTimer: ReturnType<typeof setTimeout> | undefined

function showToast(message: string, tone: ToastTone) {
  toast.value = message
  toastTone.value = tone
  if (toastTimer) clearTimeout(toastTimer)
  toastTimer = setTimeout(() => {
    toast.value = ''
  }, 3500)
}

function dismissToast() {
  if (toastTimer) clearTimeout(toastTimer)
  toast.value = ''
}

// 点击进入项目列表：只有绿点（完全可用）状态才允许导航；
// 其余状态弹 toast 说明原因 —— 原因判断与 dotClass 同源，按其
// 优先级顺序细分（offline 灰 / danger 红两种 / warn 黄两种）。
function onDeviceClick(d: UserDevice) {
  const name = displayName(d)
  const tone = dotClass(d)
  if (tone === 'offline') {
    showToast(`设备「${name}」状态探测中或未知，请稍候重试`, 'muted')
    return
  }
  if (tone === 'danger') {
    if (!isOnline(d)) {
      showToast(`设备「${name}」未连接上（离线），无法进入项目列表`, 'danger')
    } else {
      showToast(`设备「${name}」的 rathole 未运行，隧道未连接，无法进入项目列表`, 'danger')
    }
    return
  }
  if (tone === 'warn') {
    if (!d.bound) {
      showToast(`设备「${name}」未绑定，请先在设备端完成绑定`, 'warn')
    } else {
      showToast(`设备「${name}」的 oc serve 未启动，请在设备端启动后重试`, 'warn')
    }
    return
  }
  // 绿点：完全可用。保留旧代码的 os 兜底（状态快照缺 os 时无法
  // 构造合法跳转路径，不属导航地址变更）。
  const os = statusOf(d)?.status?.system?.os
  if (os !== 'macos' && os !== 'windows') {
    showToast(`设备「${name}」系统类型不支持，无法进入项目列表`, 'muted')
    return
  }
  router.push(`/devices/${os}/${encodeURIComponent(d.name)}/projects`)
}

onMounted(() => {
  silentFirstLoad()
})
</script>

<style scoped>
.page-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
}
.page-head h1 {
  margin: 0;
  font-size: 1.4rem;
}
.refresh-btn {
  min-height: 40px;
  padding: 8px 16px;
  white-space: nowrap;
}
.device-card {
  display: grid;
  grid-template-columns: 1fr;
  gap: 12px;
  align-items: center;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 16px;
  margin: 8px 0;
  color: var(--text);
  text-decoration: none !important;
  transition: transform 0.1s, box-shadow 0.1s;
  min-height: 44px;
}
.device-card:hover {
  transform: translateY(-1px);
  box-shadow: var(--shadow);
}
.device-card.card-disabled {
  opacity: 0.75;
}
.device-main {
  color: var(--text);
  text-decoration: none !important;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
/* 行1 头部：点 + 设备名 + 绑定徽章。设备名可伸缩并强制折行，
   绑定徽章固定不缩、始终右对齐 —— 无论名字多长头部都不溢出。 */
.card-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.card-head .dot {
  flex: 0 0 auto;
}
/* 状态点扩展色：红 = 未连接上，黄 = 已连上但不可用（绿/灰为全局类） */
.card-head .dot.warn {
  background: var(--warn);
}
.card-head .dot.danger {
  background: var(--danger);
}
.card-head .name {
  flex: 1 1 auto;
  min-width: 0;
  font-size: 1.1rem;
  line-height: 1.35;
  overflow-wrap: anywhere;
}
.chip.bound {
  flex: 0 0 auto;
  margin-left: auto;
  white-space: nowrap;
}
/* 行2 元信息徽章容器：横向排列，放不下自动换到下一行 */
.card-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
/* 徽章（原 pctype 演进）：药丸样式；max-width + overflow-wrap 保证
   超长服务名在徽章内部折行而不是撑破卡片 */
.chip {
  display: inline-block;
  max-width: 100%;
  min-width: 0;
  font-size: 0.75rem;
  line-height: 1.5;
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--bg);
  color: var(--text-muted);
  overflow-wrap: anywhere;
}
.chip.port {
  white-space: nowrap;
}
.chip.service {
  flex: 0 1 auto;
}
/* 平台类型 chip：首字母大写展示（windows → Windows） */
.chip.platform {
  text-transform: capitalize;
}
/* 绑定状态徽章：已绑定 = 成功色，未绑定 = 弱化灰 */
.chip.bound-on {
  background: rgba(47, 133, 90, 0.12);
  color: var(--success);
}
.chip.bound-off {
  background: var(--bg);
  color: var(--text-muted);
}
.device-card .small {
  font-size: 0.8rem;
}
/* 行4 云隧道地址：等宽字体，长地址按字符折行 */
.device-card .path {
  color: var(--text-muted);
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.75rem;
  word-break: break-all;
}
.empty {
  text-align: center;
}
.ok-text {
  color: var(--success);
}
.warn-text {
  color: var(--warn);
}
/* 点击拦截 Toast：页面级浮层，沿用项目卡片视觉语言；
   左边框颜色提示状态级别（红/黄/灰）。 */
.toast {
  position: fixed;
  top: 24px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 100;
  max-width: min(420px, calc(100vw - 32px));
  padding: 10px 14px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-left: 3px solid var(--text-muted);
  border-radius: var(--radius);
  box-shadow: var(--shadow);
  color: var(--text);
  font-size: 0.8rem;
  line-height: 1.5;
  cursor: pointer;
  overflow-wrap: anywhere;
}
.toast.tone-danger {
  border-left-color: var(--danger);
}
.toast.tone-warn {
  border-left-color: var(--warn);
}
.toast.tone-muted {
  border-left-color: var(--text-muted);
}
/* 进出场过渡：仅 opacity/transform（GPU 合成），8px 下滑淡入淡出 */
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.2s, transform 0.2s;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-8px);
}
</style>
