<template>
  <div v-if="visible" class="overlay" @click.self="$emit('close')">
    <div class="card dialog">
      <h3>用户设置</h3>

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

      <!-- 云服务设置 -->
      <form @submit.prevent="saveIp">
        <label>
          <span class="lbl">云服务 IP</span>
          <input v-model.trim="ipDraft" type="text" maxlength="64" placeholder="8.159.159.138" />
        </label>
        <div class="row action-row">
          <button type="submit" class="primary" :disabled="savingIp">
            {{ savingIp ? '保存中…' : '保存 IP' }}
          </button>
        </div>
      </form>

      <!-- SB 配置（域名/账号/密码，自动填充管理员分配值，可修改） -->
      <form @submit.prevent="saveSb">
        <span class="lbl">SB 配置</span>
        <label>
          <span class="lbl sub">域名</span>
          <input v-model.trim="sbUrlDraft" type="text" maxlength="200" placeholder="https://md.isoops.com" />
        </label>
        <label>
          <span class="lbl sub">账号</span>
          <input v-model.trim="sbUserDraft" type="text" maxlength="64" placeholder="SB 账号" autocomplete="off" />
        </label>
        <label>
          <span class="lbl sub">密码</span>
          <input v-model.trim="sbPassDraft" type="password" maxlength="128" placeholder="SB 密码" autocomplete="new-password" />
        </label>
        <span class="muted helper">已自动填充管理员分配的 SB 配置（存储于远程注册表）；如需调整可修改后保存。</span>
        <div class="row action-row">
          <button type="submit" class="primary" :disabled="savingSb">
            {{ savingSb ? '保存中…' : '保存 SB 配置' }}
          </button>
        </div>
      </form>

      <!-- 设备清单（只读：由管理员授权，用户不可修改） -->
      <div class="field">
        <span class="lbl">设备清单</span>
        <table class="devices-table">
          <thead>
            <tr>
              <th>设备名称</th>
              <th>描述</th>
              <th>平台</th>
              <th>服务名称</th>
              <th>端口号</th>
              <th>绑定状态</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(d, i) in me?.devices ?? []" :key="i">
              <td>{{ d['device-name'] || '—' }}</td>
              <td>{{ d.desc || '—' }}</td>
              <td>{{ d.pctype || '—' }}</td>
              <td>{{ d.name }}</td>
              <td>{{ d.port }}</td>
              <td :class="d.bound ? 'bound-on' : 'bound-off'">
                {{ d.bound ? '已绑定' : '未绑定' }}
              </td>
            </tr>
            <tr v-if="(me?.devices ?? []).length === 0">
              <td colspan="6" class="muted empty-cell">暂无设备</td>
            </tr>
          </tbody>
        </table>
        <span class="muted helper">设备清单由管理员授权，不可修改；如需变更请联系管理员。</span>
      </div>

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
const ipDraft = ref('')
const sbUrlDraft = ref('')
const sbUserDraft = ref('')
const sbPassDraft = ref('')
const error = ref('')
const notice = ref('')
const savingName = ref(false)
const savingIp = ref(false)
const savingSb = ref(false)

// 打开时用当前用户信息初始化草稿
watch(
  () => props.visible,
  (v) => {
    if (!v) return
    error.value = ''
    notice.value = ''
    nameDraft.value = me.value?.name ?? ''
    ipDraft.value = me.value?.cloud_ip ?? ''
    sbUrlDraft.value = me.value?.sb?.base_url ?? 'https://md.isoops.com'
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

async function saveIp() {
  error.value = ''
  notice.value = ''
  savingIp.value = true
  try {
    await apiPost('/api/me/cloud-ip', { ip: ipDraft.value })
    await auth.probe()
    notice.value = '云服务 IP 已更新。'
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    savingIp.value = false
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
.lbl.sub {
  font-size: 0.8rem;
  font-weight: 500;
}
.value {
  font-size: 0.9rem;
  color: var(--text);
  word-break: break-all;
}
textarea {
  resize: vertical;
}
/* 设备清单只读表格（六列，紧凑字号适配弹框宽度） */
.devices-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.78rem;
}
.devices-table th,
.devices-table td {
  border: 1px solid var(--border);
  padding: 5px 8px;
  text-align: left;
  word-break: break-all;
}
.devices-table th {
  color: var(--text-muted);
  font-weight: 600;
  font-size: 0.72rem;
  background: var(--bg);
  white-space: nowrap;
}
.devices-table td {
  color: var(--text);
}
.empty-cell {
  text-align: center;
  color: var(--text-muted);
}
.bound-on {
  color: var(--success);
}
.bound-off {
  color: var(--text-muted);
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
