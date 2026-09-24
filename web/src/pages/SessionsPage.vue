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
        :href="sessionHref(s.id) || '#'"
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
import NewSessionDialog from '../components/NewSessionDialog.vue'
import { useAuthStore } from '../store'
import { apiGet, apiPost, UnauthorizedError } from '../api'
import type { DeviceDetail, SessionInfo } from '../api'
import { buildOcSessionUrl } from '../oc-url'

const route = useRoute()
const auth = useAuthStore()
const pctype = String(route.params.pctype)
const pcname = String(route.params.pcname)
const directory = String(route.query.directory ?? '')

const sessions = ref<SessionInfo[]>([])
const loading = ref(true)
const error = ref('')
const creating = ref(false)
// 设备端 opencode serve 的 Basic 凭据（预取，失败静默）。sessionHref
// 用它构造内嵌凭据的会话直达 URL（免 Basic Auth 弹窗）。
const deviceDetail = ref<DeviceDetail | null>(null)

const backToProjects = computed(
  () => `/devices/${pctype}/${encodeURIComponent(pcname)}/projects`,
)

/** 当前设备条目（admin 录入），从 auth.me 里直接拿。 */
const deviceEntry = computed(() =>
  auth.me?.devices.find((x) => x.pctype === pctype && x.name === pcname),
)

/**
 * 会话直达 URL（2026-09-24 报告格式）：URL 内嵌凭据 + 规范路由
 * `/server/{b64url(serverURL)}/session/{sid}`，浏览器自动携带
 * Authorization 头，免原生 Basic Auth 弹窗。凭据或设备 URL 未就绪
 * 时返回空串（模板降级为 '#'）。
 */
const sessionHref = (sid: string) => {
  const d = deviceEntry.value
  if (!d || !deviceDetail.value) return ''
  return buildOcSessionUrl(d, deviceDetail.value, sid)
}

async function load() {
  if (!directory) {
    error.value = 'missing ?directory='
    return
  }
  loading.value = true
  error.value = ''
  // 凭据预取与 sessions 并行，不阻塞渲染；失败静默（fallbackHref
  // 退化为无 token 裸链，行为同旧版）。
  apiGet<DeviceDetail>(
    `/api/devices/${pctype}/${encodeURIComponent(pcname)}/detail`,
  )
    .then((d) => {
      deviceDetail.value = d
    })
    .catch(() => {})
  try {
    sessions.value = await apiGet<SessionInfo[]>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/sessions?directory=${encodeURIComponent(directory)}`,
    )
  } catch (e) {
    if (e instanceof UnauthorizedError) return
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
    const r = await apiPost<{ id: string; jump_url: string }>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/sessions`,
      { directory },
    )
    nsVisible.value = false
    // 新标签页打开：优先免弹窗的自拼 URL；凭据未就绪时退回 BFF 的
    // jump_url（旧格式，可能触发 Basic Auth 弹窗，好过打不开）。
    window.open(sessionHref(r.id) || r.jump_url, '_blank', 'noopener,noreferrer')
  } catch (e) {
    if (e instanceof UnauthorizedError) return
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