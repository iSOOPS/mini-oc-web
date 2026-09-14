<template>
  <div class="has-fab">
    <div class="page-head">
      <router-link :to="backToProjects" class="back-btn">← 返回项目</router-link>
      <h1>会话</h1>
      <p class="muted path">{{ directory }}</p>
    </div>

    <p v-if="error" class="error">{{ error }}</p>

    <p v-if="loading" class="muted">加载中…</p>

    <div v-else-if="sessions.length === 0" class="card empty">
      <p>暂无会话,点下方按钮新建一个。</p>
    </div>

    <div v-else class="stack">
      <a
        v-for="s in sessions"
        :key="s.id"
        :href="jumpUrls[s.id] || fallbackHref(s.id)"
        target="_blank"
        rel="noopener"
        class="session-card"
      >
        <div class="row top">
          <strong>{{ s.title || s.id }}</strong>
          <span v-if="s.updatedAt" class="muted small">
            {{ formatTime(s.updatedAt) }}
          </span>
        </div>
        <code class="sid">{{ s.id }}</code>
      </a>
    </div>

    <button class="primary fab-btn" :disabled="creating" @click="openNewSession">
      {{ creating ? '创建中…' : '+ 新建会话' }}
    </button>

    <DeviceAuthDialog
      :visible="authVisible"
      :base-url="authBaseUrl"
      :busy="authBusy"
      :error="authError"
      @submit="submitAuth"
      @cancel="cancelAuth"
    />

    <NewSessionDialog
      :visible="nsVisible"
      :projects="[]"
      :loading-projects="false"
      :default-directory="directory"
      :busy="creating"
      :error="nsError"
      :lock-directory="true"
      @submit="createSession"
      @cancel="closeNewSession"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import DeviceAuthDialog from '../components/DeviceAuthDialog.vue'
import NewSessionDialog from '../components/NewSessionDialog.vue'
import { useDeviceAuth } from '../deviceAuth'
import { apiGet, apiPost, ApiError, DEVICE_AUTH_FAILED, UnauthorizedError } from '../api'
import type { DeviceView, SessionInfo } from '../api'

const route = useRoute()
const {
  visible: authVisible,
  busy: authBusy,
  error: authError,
  baseUrl: authBaseUrl,
  open: openAuth,
  submit: submitAuth,
  cancel: cancelAuth,
} = useDeviceAuth()
const pctype = String(route.params.pctype)
const pcname = String(route.params.pcname)
const directory = String(route.query.directory ?? '')

const sessions = ref<SessionInfo[]>([])
const loading = ref(true)
const error = ref('')
const creating = ref(false)
const device = ref<DeviceView | null>(null)
// sid -> BFF /jump 生成的跳转 URL；未拿到前用 fallbackHref 降级。
const jumpUrls = ref<Record<string, string>>({})

const backToProjects = computed(
  () => `/devices/${pctype}/${encodeURIComponent(pcname)}/projects`,
)

// 本地 base64 计算的降级方案（与 BFF jump.rs 同一编码），/jump 请求失败时兜底。
const fallbackHref = (sid: string) => {
  if (!device.value) return '#'
  // Same encoding as BFF jump.rs: URL-safe base64 of percent-encoded directory.
  const b64dir = btoa(encodeURIComponent(directory))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  return `${device.value.portal_base}/${device.value.pcname_b64}/${b64dir}/session/${sid}`
}

async function load() {
  if (!directory) {
    error.value = 'missing ?directory='
    return
  }
  loading.value = true
  error.value = ''
  try {
    // Pull device list so we have pcname_b64 + portal_base for jump URLs.
    const list = await apiGet<DeviceView[]>('/api/devices')
    device.value =
      list.find((d) => d.pctype === pctype && d.pcname === pcname) ?? null
    sessions.value = await apiGet<SessionInfo[]>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/sessions?directory=${encodeURIComponent(directory)}`,
    )
    // 并发为每个会话拉取 BFF /jump 生成的跳转 URL，不阻塞页面渲染；
    // 单个失败静默，模板回退到 fallbackHref。
    await Promise.all(
      sessions.value.map(async (s) => {
        try {
          const r = await apiGet<{ jump_url: string }>(
            `/api/devices/${pctype}/${encodeURIComponent(pcname)}/jump?directory=${encodeURIComponent(directory)}&session=${s.id}&format=json`,
          )
          jumpUrls.value[s.id] = r.jump_url
        } catch {
          // 静默：保留本地 fallbackHref
        }
      }),
    )
  } catch (e) {
    if (e instanceof UnauthorizedError) return
    // 设备端拒绝 BFF 凭据：弹授权框，通过后自动重载。
    if (e instanceof ApiError && e.code === DEVICE_AUTH_FAILED) {
      openAuth(pctype, pcname, device.value?.public_url ?? '', () => load())
      return
    }
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

// 新建会话：路径锁定为当前目录的二次确认（lockDirectory 模式），
// 创建后新标签页打开。
const nsVisible = ref(false)
const nsError = ref('')

function openNewSession() {
  nsError.value = ''
  nsVisible.value = true
}

function closeNewSession() {
  if (creating.value) return
  nsVisible.value = false
}

async function createSession(directory: string) {
  creating.value = true
  nsError.value = ''
  try {
    const r = await apiPost<{ jump_url: string }>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/sessions`,
      { directory },
    )
    nsVisible.value = false
    // 新标签页打开（问题1），不离开当前列表页
    window.open(r.jump_url, '_blank', 'noopener,noreferrer')
  } catch (e) {
    if (e instanceof UnauthorizedError) return
    if (e instanceof ApiError && e.code === DEVICE_AUTH_FAILED) {
      openAuth(pctype, pcname, device.value?.public_url ?? '', () => createSession(directory))
      return
    }
    nsError.value = e instanceof Error ? e.message : String(e)
  } finally {
    creating.value = false
  }
}

function formatTime(s: string): string {
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  return d.toLocaleString()
}

onMounted(load)
</script>

<style scoped>
.page-head {
  margin-bottom: 16px;
}
h1 {
  margin: 0;
  font-size: 1.3rem;
}
.path {
  margin: 4px 0 0;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.8rem;
  word-break: break-all;
}
.session-card {
  display: block;
  padding: 14px 16px;
  margin: 8px 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text);
  text-decoration: none !important;
  min-height: 44px;
}
.session-card:hover {
  box-shadow: var(--shadow);
}
.row.top {
  margin-bottom: 4px;
}
.sid {
  font-size: 0.75rem;
  color: var(--text-muted);
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
}
.small {
  font-size: 0.8rem;
}
.empty {
  text-align: center;
}
</style>