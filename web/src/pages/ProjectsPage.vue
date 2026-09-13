<template>
  <div class="has-fab">
    <div class="page-head">
      <div class="head-row">
        <router-link to="/devices" class="back-btn">← 返回设备</router-link>
        <input
          v-model.trim="keyword"
          type="search"
          class="search"
          placeholder="搜索路径关键字…"
          aria-label="搜索项目路径"
        />
      </div>
      <h1>{{ pctype }} / {{ pcname }} 的项目</h1>
    </div>

    <p v-if="error" class="error">{{ error }}</p>
    <p v-if="loading" class="muted">加载中…</p>

    <div v-else-if="projects.length === 0" class="card empty">
      <p>该项目下还没有项目。</p>
    </div>

    <div v-else-if="filteredProjects.length === 0" class="card empty">
      <p>没有匹配「{{ keyword }}」的项目。</p>
    </div>

    <div v-else class="stack">
      <router-link
        v-for="p in filteredProjects"
        :key="p.path"
        :to="`/devices/${pctype}/${encodeURIComponent(pcname)}/sessions?directory=${encodeURIComponent(p.path)}`"
        class="project-card"
      >
        <strong class="path">{{ p.path }}</strong>
        <span v-if="p.lastOpenedAt" class="muted small">
          最后打开 {{ formatTime(p.lastOpenedAt) }}
        </span>
      </router-link>
    </div>

    <button class="primary fab-btn" :disabled="creating" @click="openNewProject">
      {{ creating ? '创建中…' : '+ 新建项目' }}
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
      :projects="nsProjects"
      :loading-projects="nsLoading"
      :default-directory="''"
      :busy="creating"
      :error="nsError"
      @submit="createProject"
      @cancel="closeNewProject"
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
import type { ProjectInfo } from '../api'

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
const projects = ref<ProjectInfo[]>([])
const loading = ref(true)
const error = ref('')

// 路径关键字实时过滤（本地，无需确认）：大小写不敏感的子串匹配。
const keyword = ref('')
const filteredProjects = computed(() => {
  const kw = keyword.value.toLowerCase()
  if (!kw) return projects.value
  return projects.value.filter((p) => p.path.toLowerCase().includes(kw))
})

async function load() {
  loading.value = true
  error.value = ''
  try {
    projects.value = await apiGet<ProjectInfo[]>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/projects`,
    )
  } catch (e) {
    // 401 由 router 守卫处理（软跳 /login），不显示错误
    if (e instanceof UnauthorizedError) return
    // 设备端拒绝 BFF 凭据：弹授权框，通过后自动重载
    // （直接 URL 访问本页时不会经过 DevicesPage 的点击探测）。
    if (e instanceof ApiError && e.code === DEVICE_AUTH_FAILED) {
      openAuth(pctype, pcname, '', () => load())
      return
    }
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

function formatTime(s: string): string {
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  return d.toLocaleString()
}

// 新建项目：与"新建会话"同弹框、同创建流程（POST sessions → 新标签页
// 打开），区别是这里允许选择/输入任意路径；列表刷新以反映新项目
// （新目录需端侧同步后出现）。
const nsVisible = ref(false)
const nsProjects = ref<ProjectInfo[]>([])
const nsLoading = ref(false)
const nsError = ref('')
const creating = ref(false)

function openNewProject() {
  nsError.value = ''
  nsVisible.value = true
  if (nsProjects.value.length === 0 && !nsLoading.value) {
    nsLoading.value = true
    apiGet<ProjectInfo[]>(`/api/devices/${pctype}/${encodeURIComponent(pcname)}/projects`)
      .then((ps) => {
        nsProjects.value = ps
      })
      .catch(() => {
        // 项目列表加载失败不阻塞：用户仍可手动输入路径
      })
      .finally(() => {
        nsLoading.value = false
      })
  }
}

function closeNewProject() {
  if (creating.value) return
  nsVisible.value = false
}

async function createProject(directory: string) {
  creating.value = true
  nsError.value = ''
  try {
    const r = await apiPost<{ jump_url: string }>(
      `/api/devices/${pctype}/${encodeURIComponent(pcname)}/sessions`,
      { directory },
    )
    nsVisible.value = false
    window.open(r.jump_url, '_blank', 'noopener,noreferrer')
    await load()
  } catch (e) {
    if (e instanceof UnauthorizedError) return
    if (e instanceof ApiError && e.code === DEVICE_AUTH_FAILED) {
      openAuth(pctype, pcname, '', () => createProject(directory))
      return
    }
    nsError.value = e instanceof Error ? e.message : String(e)
  } finally {
    creating.value = false
  }
}

onMounted(load)
</script>

<style scoped>
.page-head {
  margin-bottom: 16px;
}
.head-row {
  display: grid;
  grid-template-columns: 1fr 2fr;
  gap: 8px;
  align-items: stretch;
  margin-bottom: 12px;
}
.head-row .back-btn {
  margin-bottom: 0;
}
.search {
  min-width: 0;
}
@media (max-width: 480px) {
  .head-row {
    grid-template-columns: 1fr;
  }
}
h1 {
  margin: 0;
  font-size: 1.25rem;
  word-break: break-all;
}
.project-card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 14px 16px;
  margin: 8px 0;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  background: var(--surface);
  color: var(--text);
  text-decoration: none !important;
  min-height: 44px;
}
.project-card:hover {
  box-shadow: var(--shadow);
}
.path {
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.9rem;
  word-break: break-all;
}
.small {
  font-size: 0.8rem;
}
.empty {
  text-align: center;
}
</style>