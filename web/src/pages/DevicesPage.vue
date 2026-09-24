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
            <span class="chip port">服务 {{ d.port }}</span>
            <span class="chip port">oc {{ d['oc-port'] ?? 9464 }}</span>
          </div>
          <!-- 行3 可用状态徽章（按 -1/0/1 红/橙/绿） -->
          <div class="card-status small">
            <span class="chip" :class="availableChip(d)">
              可用：{{ availableText(d) }}
            </span>
          </div>
          <!-- 行4 在线状态 / opencode 状态 -->
          <div class="card-status muted small">
            <div v-if="d.online !== undefined">
              <span :class="d.online ? 'ok-text' : 'danger-text'">
                在线 {{ d.online ? '是' : '否' }}
              </span>
              <template v-if="d.online"> · </template>
              <template v-if="d.online">
                <span :class="d.opencode_online ? 'ok-text' : 'warn-text'">
                  opencode {{ d.opencode_online ? '在线' : '离线' }}
                </span>
              </template>
              <template v-if="d.online && hostOf(d)"> · {{ hostOf(d) }}</template>
              <span v-if="!d.online && d.reason" class="warn-text"> · {{ d.reason }}</span>
            </div>
            <div v-else>状态未知</div>
          </div>
          <!-- 行4 设备穿透地址（admin 配置，BFF 以 穿透地址+服务端口号 探测，
               以 穿透地址+OpenCode 端口号 跳转） -->
          <div class="path">
            {{ d['public-url'] ? `${d['public-url']} · 服务:${d.port} · oc:${d['oc-port'] ?? 9464}` : '设备穿透地址未配置' }}
          </div>
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
import { UnauthorizedError } from '../api'
import type { UserDevice } from '../api'
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
  refreshing.value = { done: 0, total: 1 }   // 单次 /api/me 调用

  try {
    try {
      await auth.probe(ctrl.signal)
    } catch (e) {
      if (e instanceof UnauthorizedError) return
      showToast('刷新设备清单失败，请稍后重试', 'warn')
      return
    }
    if (ctrl.signal.aborted) return
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
}

// 用户设备清单（管理员授权）直接驱动卡片;状态通过 admin 配置的设备 URL 主动探测。
const devices = computed(() => auth.me?.devices ?? [])

/** 卡片主标题：设备名称（客户端绑定写入）→ 描述 → 服务名。 */
function displayName(d: UserDevice): string {
  return d['device-name'] || d.desc || d.name
}

function isOnline(d: UserDevice): boolean {
  return d.online === true
}
function rathole(d: UserDevice): string {
  return d.status?.rathole ?? 'unknown'
}
function hostOf(d: UserDevice): string {
  return d.status?.system?.hostname ?? ''
}

/** 可用状态文案（-1/0/1 → 不可用 / 部分可用 / 完全可用）。 */
function availableText(d: UserDevice): string {
  switch (d.available) {
    case 1:
      return '完全可用'
    case 0:
      return '部分可用'
    case -1:
      return '不可用'
    default:
      return '未知'
  }
}

/** 可用状态徽章样式类（与点颜色保持一致：红/橙/绿）。 */
function availableChip(d: UserDevice): string {
  switch (d.available) {
    case 1:
      return 'avail-on'
    case 0:
      return 'avail-warn'
    case -1:
      return 'avail-off'
    default:
      return ''
  }
}

/**
 * 状态点颜色（直接由后端的 `available` 驱动）：
 * 绿 = 1（完全可用）;橙 = 0（已连接但未完全可用）;红 = -1（不可用）
 * 数据未到时灰显，避免页面刚加载瞬间闪红/橙。
 */
function dotClass(d: UserDevice): string {
  if (d.available === undefined) return 'offline'
  if (d.available === 1) return 'online'
  if (d.available === 0) return 'warn'
  return 'danger'
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
// 优先级顺序细分（offline 灰 / danger 红 / warn 橙）。
function onDeviceClick(d: UserDevice) {
  const name = displayName(d)
  const tone = dotClass(d)
  if (tone === 'offline') {
    showToast(`设备「${name}」状态探测中或未知，请稍候重试`, 'muted')
    return
  }
  if (tone === 'danger') {
    showToast(`设备「${name}」不可用（/status 探测失败），无法进入项目列表`, 'danger')
    return
  }
  if (tone === 'warn') {
    if (!d.bound) {
      showToast(`设备「${name}」未绑定，请先在设备端完成绑定`, 'warn')
    } else {
      showToast(`设备「${name}」的 opencode 进程未运行，请在设备端启动后重试`, 'warn')
    }
    return
  }
  // 绿点：完全可用。保留旧代码的 os 兜底（状态快照缺 os 时无法
  // 构造合法跳转路径，不属导航地址变更）。
  const os = d.status?.system?.os
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
.danger-text {
  color: var(--danger);
}
/* 可用状态徽章：与点色保持一致（绿/橙/红）；沿用 chip 药丸样式 */
.chip.avail-on {
  background: rgba(47, 133, 90, 0.12);
  color: var(--success);
}
.chip.avail-warn {
  background: rgba(217, 119, 6, 0.12);
  color: var(--warn);
}
.chip.avail-off {
  background: rgba(220, 38, 38, 0.12);
  color: var(--danger);
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
