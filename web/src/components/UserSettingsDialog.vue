<template>
  <div v-if="visible" class="overlay" @click.self="$emit('close')">
    <div class="card dialog">
      <h3>设置</h3>

      <!-- 用户信息卡片：唯一 ID / 用户名 / 登录密钥 / 上次使用 -->
      <section class="section-card">
        <span class="section-title">用户信息</span>

        <!-- 唯一 ID（只读，创建时生成的 6 位随机数） -->
        <div class="field">
          <span class="lbl">唯一 ID</span>
          <div class="key-row">
            <code class="key-text">{{ me?.id ?? '—' }}</code>
            <button class="ghost small-btn" @click="copyId">
              {{ idCopied ? '已复制' : '复制' }}
            </button>
          </div>
          <span class="muted helper">唯一 ID 由系统分配，不可修改。</span>
        </div>

        <!-- 修改用户名 -->
        <form @submit.prevent="saveName">
          <label>
            <span class="lbl">用户名</span>
            <input v-model.trim="nameDraft" type="text" maxlength="64" placeholder="字母/数字/_/-，1-64 位" />
          </label>
          <div class="row action-row">
            <button type="submit" class="primary" :disabled="savingName">
              {{ savingName ? '保存中…' : '修改用户名' }}
            </button>
          </div>
        </form>

        <!-- 查看用户密码（登录密钥） -->
        <div class="field">
          <span class="lbl">用户密码（登录密钥）</span>
          <div class="key-row">
            <code class="key-text">{{ keyRevealed ? myKey : '•'.repeat(32) }}</code>
            <button class="ghost small-btn" @click="toggleKey">
              {{ keyRevealed ? '隐藏' : '显示' }}
            </button>
            <button v-if="keyRevealed" class="ghost small-btn" @click="copyKey">
              {{ copied ? '已复制' : '复制' }}
            </button>
          </div>
        </div>

        <!-- 上次使用时间 -->
        <div class="field">
          <span class="lbl">上次使用时间</span>
          <div class="value">{{ me?.last_used_at ?? '—' }}</div>
        </div>
      </section>

      <!-- SB 配置卡片（域名/账号/密码，自动填充管理员分配值，可修改） -->
      <section class="section-card">
        <span class="section-title">SB 配置</span>
        <form @submit.prevent="saveSb">
          <label>
            <span class="lbl">域名</span>
            <input v-model.trim="sbUrlDraft" type="text" maxlength="200" placeholder="https://sb.example.com" />
          </label>
          <label>
            <span class="lbl">账号</span>
            <input v-model.trim="sbUserDraft" type="text" maxlength="64" placeholder="SB 账号" autocomplete="off" />
          </label>
          <label>
            <span class="lbl">密码</span>
            <input v-model.trim="sbPassDraft" type="password" maxlength="128" placeholder="SB 密码" autocomplete="new-password" />
          </label>
          <span class="muted helper">已自动填充管理员分配的 SB 配置（存储于远程注册表）；如需调整可修改后保存。</span>
          <div class="row action-row">
            <button type="submit" class="primary" :disabled="savingSb">
              {{ savingSb ? '保存中…' : '保存 SB 配置' }}
            </button>
          </div>
        </form>
      </section>

      <!-- 设备清单卡片（只读：由管理员授权，用户不可修改）。
           移动端友好：每台设备一张子卡片，标题行 = 设备名 + 可用状态
           徽章，其余字段按 标签/值 网格自适应排布（窄屏单列、宽屏多列）。 -->
      <section class="section-card">
        <span class="section-title">设备清单</span>
        <div class="device-cards">
          <div v-for="(d, i) in me?.devices ?? []" :key="i" class="device-card">
            <div class="device-head">
              <strong class="device-title">{{ d['device-name'] || d.desc || d.name }}</strong>
              <span class="pill" :class="availableClass(d.available)">
                {{ availableText(d.available) }}
              </span>
            </div>
            <dl class="device-grid">
              <div class="cell">
                <dt>描述</dt>
                <dd>{{ d.desc || '—' }}</dd>
              </div>
              <div class="cell">
                <dt>平台</dt>
                <dd>{{ d.pctype || '—' }}</dd>
              </div>
              <div class="cell">
                <dt>服务名称</dt>
                <dd>{{ d.name }}</dd>
              </div>
              <div class="cell wide">
                <dt>穿透地址</dt>
                <dd>{{ d['public-url'] || '—' }}</dd>
              </div>
              <div class="cell">
                <dt>服务端口</dt>
                <dd>{{ d.port }}</dd>
              </div>
              <div class="cell">
                <dt>oc 端口</dt>
                <dd>{{ d['oc-port'] ?? '—' }}</dd>
              </div>
              <div class="cell">
                <dt>绑定状态</dt>
                <dd :class="d.bound ? 'state-on' : 'state-off'">
                  {{ d.bound ? '已绑定' : '未绑定' }}
                </dd>
              </div>
              <div class="cell">
                <dt>在线</dt>
                <dd :class="stateClass(d.online)">{{ stateText(d.online) }}</dd>
              </div>
              <div class="cell">
                <dt>opencode</dt>
                <dd :class="stateClass(d.opencode_online)">
                  {{ stateText(d.opencode_online) }}
                </dd>
              </div>
            </dl>
          </div>
          <p v-if="(me?.devices ?? []).length === 0" class="muted empty-cell">暂无设备</p>
        </div>
        <span class="muted helper">设备清单由管理员授权，不可修改；如需变更请联系管理员。</span>
      </section>

      <p v-if="error" class="error">{{ error }}</p>
      <p v-if="notice" class="muted notice">{{ notice }}</p>

      <button class="ghost close-btn" @click="$emit('close')">关闭</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { apiGet, apiPost } from '../api'
import { useAuthStore } from '../store'

const props = defineProps<{ visible: boolean }>()
defineEmits<{ (e: 'close'): void }>()

const auth = useAuthStore()
const me = computed(() => auth.me)

const nameDraft = ref('')
const sbUrlDraft = ref('')
const sbUserDraft = ref('')
const sbPassDraft = ref('')
const error = ref('')
const notice = ref('')
const savingName = ref(false)
const savingSb = ref(false)

function stateText(v: boolean | undefined): string {
  if (v === undefined) return '—'
  return v ? '是' : '否'
}

function stateClass(v: boolean | undefined): string {
  if (v === undefined) return ''
  return v ? 'state-on' : 'state-off'
}

function availableText(v: number | undefined): string {
  switch (v) {
    case 1:
      return '完全可用'
    case 0:
      return '部分可用'
    case -1:
      return '不可用'
    default:
      return '—'
  }
}

function availableClass(v: number | undefined): string {
  switch (v) {
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

// 打开时用当前用户信息初始化草稿
watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    notice.value = ''
    nameDraft.value = me.value?.name ?? ''
    sbUrlDraft.value = me.value?.sb?.base_url ?? 'http://127.0.0.1:3000'
    sbUserDraft.value = me.value?.sb?.username ?? ''
    sbPassDraft.value = me.value?.sb?.password ?? ''
    keyRevealed.value = false
    myKey.value = ''
  },
)

async function saveName() {
  error.value = ''
  notice.value = ''
  savingName.value = true
  try {
    await apiPost('/api/me/name', { name: nameDraft.value })
    await auth.probe()
    notice.value = '用户名已更新。'
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    savingName.value = false
  }
}

async function saveSb() {
  error.value = ''
  notice.value = ''
  savingSb.value = true
  try {
    await apiPost('/api/me/sb', {
      base_url: sbUrlDraft.value,
      username: sbUserDraft.value,
      password: sbPassDraft.value,
    })
    await auth.probe()
    notice.value = 'SB 配置已更新。'
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    savingSb.value = false
  }
}

// --- 唯一 ID 复制 ---
const idCopied = ref(false)
let idCopyTimer: number | undefined

async function copyId() {
  try {
    await navigator.clipboard.writeText(me.value?.id ?? '')
    idCopied.value = true
    if (idCopyTimer !== undefined) window.clearTimeout(idCopyTimer)
    idCopyTimer = window.setTimeout(() => {
      idCopied.value = false
    }, 1500)
  } catch {
    // 剪贴板不可用时静默忽略
  }
}

// --- 密钥查看（懒加载：首次点显示时才请求） ---
const keyRevealed = ref(false)
const myKey = ref('')
let copyTimer: number | undefined
const copied = ref(false)

async function toggleKey() {
  if (keyRevealed.value) {
    keyRevealed.value = false
    return
  }
  if (!myKey.value) {
    try {
      const r = await apiGet<{ key: string }>('/api/me/key')
      myKey.value = r.key
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return
    }
  }
  keyRevealed.value = true
}

async function copyKey() {
  try {
    await navigator.clipboard.writeText(myKey.value)
    copied.value = true
    if (copyTimer !== undefined) window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copied.value = false
    }, 1500)
  } catch {
    // 剪贴板不可用时静默忽略
  }
}
</script>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 16px;
  z-index: 100;
}
.dialog {
  width: 100%;
  max-width: 440px;
  margin: 0;
  max-height: 90vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.dialog h3 {
  margin: 0;
  font-size: 1.1rem;
}
form,
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
/* 分组卡片：浅色底 + 边框，包裹用户信息 / SB 配置 / 设备清单 */
.section-card {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.section-card > .section-title {
  font-size: 0.9rem;
  font-weight: 700;
  color: var(--text);
}
.section-card form {
  gap: 10px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.lbl {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-muted);
}
.value {
  font-size: 0.9rem;
  color: var(--text);
  word-break: break-all;
}
textarea {
  resize: vertical;
}
/* 设备清单：每台设备一张子卡片（surface 底与 section-card 的 bg 区分层级）。
   字段网格 auto-fill 自适应——移动端单列、宽屏多列；穿透地址等长值
   占满整行（wide）。 */
.device-cards {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.device-card {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.device-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  flex-wrap: wrap;
}
.device-title {
  font-size: 0.95rem;
  word-break: break-all;
}
.pill {
  font-size: 0.75rem;
  line-height: 1.5;
  padding: 2px 10px;
  border-radius: 10px;
  white-space: nowrap;
}
.pill.avail-on {
  background: rgba(47, 133, 90, 0.12);
  color: var(--success);
}
.pill.avail-warn {
  background: rgba(234, 179, 8, 0.15);
  color: var(--warn);
}
.pill.avail-off {
  background: rgba(197, 48, 48, 0.12);
  color: var(--danger);
}
.device-grid {
  margin: 0;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 8px 12px;
}
.cell dt {
  font-size: 0.72rem;
  color: var(--text-muted);
  margin-bottom: 2px;
}
.cell dd {
  margin: 0;
  font-size: 0.82rem;
  color: var(--text);
  word-break: break-all;
}
.cell.wide {
  grid-column: 1 / -1;
}
.empty-cell {
  text-align: center;
  margin: 4px 0;
}
.state-on {
  color: var(--success);
}
.state-off {
  color: var(--danger);
}
.avail-on {
  color: var(--success);
  font-weight: 600;
}
.avail-warn {
  color: var(--warn);
  font-weight: 600;
}
.avail-off {
  color: var(--danger);
  font-weight: 600;
}
.action-row {
  margin-top: 4px;
}
.key-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.key-text {
  flex: 1;
  min-width: 120px;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 0.8rem;
  background: var(--bg);
  padding: 6px 10px;
  border-radius: var(--radius);
  word-break: break-all;
}
.small-btn {
  min-height: 32px;
  padding: 4px 10px;
  font-size: 0.8rem;
  white-space: nowrap;
}
.helper {
  font-size: 0.75rem;
}
.notice {
  margin: 0;
  font-size: 0.8rem;
  color: var(--success);
}
.close-btn {
  margin-top: 4px;
}
</style>
