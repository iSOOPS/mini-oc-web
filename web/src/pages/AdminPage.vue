<template>
  <div>
    <div class="page-head">
      <h1>管理后台</h1>
      <button class="ghost" @click="onLogout">登出</button>
    </div>

    <!-- 服务信息 -->
    <section>
      <h2 class="section-title">服务信息</h2>
      <div v-if="admin.info" class="info-grid">
        <div v-for="t in infoTiles" :key="t.label" class="card info-tile">
          <span class="info-label">{{ t.label }}</span>
          <span class="info-value">{{ t.value }}</span>
        </div>
      </div>
      <p v-else class="muted">加载中…</p>

      <!-- 敏感密钥：默认同长度 *，点击显示 -->
      <div v-if="admin.info" class="secret-grid">
        <div class="card secret-card">
          <span class="info-label">SB 密码</span>
          <div class="key-row">
            <code class="key-text">{{ secretRevealed.sb ? admin.info.sb_password : mask(admin.info.sb_password) }}</code>
            <button class="ghost small-btn" @click="toggleSecret('sb')">
              {{ secretRevealed.sb ? '隐藏' : '显示' }}
            </button>
            <button class="ghost small-btn" @click="copyKey(admin.info.sb_password, 'sb')">
              {{ copied === 'sb' ? '已复制' : '复制' }}
            </button>
          </div>
        </div>
        <div class="card secret-card">
          <span class="info-label">云服务密钥</span>
          <div class="key-row">
            <code class="key-text">{{ secretRevealed.rathole ? admin.info.rathole_key : mask(admin.info.rathole_key) }}</code>
            <button class="ghost small-btn" @click="toggleSecret('rathole')">
              {{ secretRevealed.rathole ? '隐藏' : '显示' }}
            </button>
            <button class="ghost small-btn" @click="copyKey(admin.info.rathole_key, 'rathole')">
              {{ copied === 'rathole' ? '已复制' : '复制' }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 用户管理 -->
    <section>
      <div class="toolbar">
        <h2 class="section-title">用户</h2>
        <button class="primary" @click="openCreate">+ 新建用户</button>
      </div>

      <p v-if="error" class="error">
        {{ error }}
        <button class="ghost small-btn retry-btn" @click="refresh">重试</button>
      </p>
      <p v-if="listLoading" class="muted">加载中…</p>

      <div v-else-if="admin.users.length === 0" class="card empty">
        <p class="muted">还没有用户，点击「+ 新建用户」创建第一个用户。</p>
      </div>

      <div v-else class="stack">
        <div v-for="u in admin.users" :key="u.id" class="card user-card">
          <div class="row user-head">
            <strong class="uname">{{ u.name }}</strong>
          </div>
          <div class="key-row">
            <code class="key-text">{{ revealed[u.id] ? u.key : '••••••••' }}</code>
            <button class="ghost small-btn" @click="toggleReveal(u.id)">
              {{ revealed[u.id] ? '隐藏' : '显示' }}
            </button>
            <button class="ghost small-btn" @click="copyKey(u.key, 'u:' + u.id)">
              {{ copied === 'u:' + u.id ? '已复制' : '复制' }}
            </button>
          </div>
          <p class="muted">设备清单: {{ fmtDevices(u) }}</p>
          <p v-if="u.updated_at" class="muted small-text">{{ u.updated_at }}</p>
          <div class="actions">
            <button class="ghost small-btn" :disabled="busyId === u.id" @click="openEdit(u)">
              编辑
            </button>
            <button
              class="ghost small-btn"
              :disabled="busyId === u.id"
              @click="onRegenerate(u)"
            >
              重新生成密钥
            </button>
            <button
              class="ghost small-btn del-btn"
              :disabled="busyId === u.id"
              @click="onDelete(u)"
            >
              {{ busyId === u.id ? '处理中…' : '删除' }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- 新建 / 编辑用户弹框 -->
    <div v-if="dialogMode" class="overlay" @click.self="closeForm">
      <div class="card dialog">
        <h3>{{ dialogMode === 'create' ? '新建用户' : '编辑用户' }}</h3>
        <form @submit.prevent="submitForm">
          <label>
            <span class="lbl">名称</span>
            <input
              v-model.trim="formName"
              type="text"
              placeholder="如 alice"
              maxlength="64"
              autofocus
            />
          </label>
          <!-- SB存储配置分配（域名/账号/密码，随用户存储到远程注册表） -->
          <div class="sb-field">
            <span class="lbl">SB存储配置分配</span>
            <label>
              <span class="lbl sub">域名</span>
              <input
                v-model.trim="formSbUrl"
                type="text"
                maxlength="200"
                placeholder="https://md.isoops.com"
              />
            </label>
            <label>
              <span class="lbl sub">账号</span>
              <input
                v-model.trim="formSbUser"
                type="text"
                maxlength="64"
                placeholder="SB 账号"
                autocomplete="off"
              />
            </label>
            <label>
              <span class="lbl sub">密码</span>
              <div class="sb-pass-row">
                <input
                  v-model.trim="formSbPass"
                  :type="sbPassVisible ? 'text' : 'password'"
                  maxlength="128"
                  placeholder="SB 密码"
                  autocomplete="new-password"
                />
                <button
                  type="button"
                  class="ghost small-btn pass-btn"
                  @click="sbPassVisible = !sbPassVisible"
                >
                  {{ sbPassVisible ? '隐藏' : '显示' }}
                </button>
                <button
                  v-if="sbPassVisible"
                  type="button"
                  class="ghost small-btn pass-btn"
                  @click="copyKey(formSbPass, 'sb-pass')"
                >
                  {{ copied === 'sb-pass' ? '已复制' : '复制' }}
                </button>
              </div>
            </label>
            <span class="muted helper">
              分配给该用户的 SB 存储配置（域名默认 https://md.isoops.com）；用户登录后可在用户设置中查看并按需修改。
            </span>
          </div>

          <div class="devices-field">
            <span class="lbl">设备清单</span>
            <div class="device-rows">
              <div v-for="(d, i) in formDevices" :key="i" class="device-row">
                <label>
                  <span class="lbl sub">描述</span>
                  <input
                    v-model.trim="d.desc"
                    type="text"
                    placeholder="如：办公 Mac（1-64 字符）"
                    maxlength="64"
                  />
                </label>
                <label>
                  <span class="lbl sub">服务名称</span>
                  <input
                    v-model.trim="d.name"
                    type="text"
                    placeholder="如 dev-a，仅小写字母/数字/连字符"
                    maxlength="64"
                  />
                </label>
                <label class="pctype-field">
                  <span class="lbl sub">平台类型</span>
                  <select v-model="d.pctype">
                    <option value="windows">windows</option>
                    <option value="macos">macos</option>
                  </select>
                </label>
                <label>
                  <span class="lbl sub">端口号</span>
                  <input
                    v-model.number="d.port"
                    type="number"
                    placeholder="如 4040"
                    min="1"
                    max="65535"
                  />
                </label>
                <label>
                  <span class="lbl sub">设备名称与绑定状态</span>
                  <div class="bind-row">
                    <input
                      :value="d.deviceName"
                      type="text"
                      disabled
                      placeholder="由客户端绑定时自动写入"
                    />
                    <span class="bound-pill" :class="d.bound ? 'bound-on' : 'bound-off'">
                      {{ d.bound ? '已绑定' : '未绑定' }}
                    </span>
                  </div>
                </label>
                <div class="row-actions">
                  <button type="button" class="row-act copy" @click="duplicateDeviceRow(i)">
                    复制
                  </button>
                  <button
                    type="button"
                    class="row-act del"
                    :disabled="formDevices.length <= 1"
                    @click="removeDeviceRow(i)"
                  >
                    删除
                  </button>
                </div>
              </div>
            </div>
            <button type="button" class="ghost small-btn add-row" @click="addDeviceRow">
              + 添加设备
            </button>
            <span class="muted helper">
              每台设备包含：描述（展示用，1-64 字符）、服务名称（仅限小写字母、数字、连字符，不可重复）、平台类型（windows/macos）、端口号（1-65535）、设备名称（只读）与绑定状态（均由客户端绑定时自动写入/更新，不可人工修改）；「复制」会克隆一份当前设备配置，保存时服务名称不可与其他设备重复。
            </span>
          </div>
          <p v-if="formError" class="error">{{ formError }}</p>
          <div class="row dialog-actions">
            <button type="button" class="ghost" @click="closeForm">取消</button>
            <button type="submit" class="primary" :disabled="submitting">
              {{ submitting ? '保存中…' : '保存' }}
            </button>
          </div>
        </form>
      </div>
    </div>

    <!-- 密钥结果面板（新建 / 重新生成共用） -->
    <div v-if="resultVisible" class="overlay">
      <div class="card dialog key-result">
        <h3>{{ resultTitle }}</h3>
        <code class="key-box">{{ resultKey }}</code>
        <div class="row">
          <button class="ghost small-btn" @click="copyKey(resultKey, 'result')">
            {{ copied === 'result' ? '已复制' : '复制' }}
          </button>
        </div>
        <p class="muted">请将密钥发送给该用户；密钥也可在列表中随时查看。</p>
        <button class="primary" @click="closeResult">确定</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAdminStore } from '../store'
import { UnauthorizedError } from '../api'
import type { AdminInfo, PortalUser, UserBody, UserDevice } from '../api'

const admin = useAdminStore()
const router = useRouter()

// --- 通用错误处理：401 → 软跳管理登录页 ---
function toErrorMessage(e: unknown): string {
  if (e instanceof UnauthorizedError) {
    router.push('/admin/login')
    return ''
  }
  return e instanceof Error ? e.message : String(e)
}

// --- 服务信息 ---
function fmtUptime(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const parts: string[] = []
  if (d > 0) parts.push(`${d}天`)
  if (d > 0 || h > 0) parts.push(`${h}小时`)
  parts.push(`${m}分钟`)
  return parts.join(' ')
}

/** 本机 IP 卡片仅展示 IPv4 地址（过滤掉 IPv6 与其他格式）。 */
function ipv4Only(ips: string[]): string[] {
  return ips.filter((ip) => /^\d{1,3}(\.\d{1,3}){3}$/.test(ip))
}

const infoTiles = computed<{ label: string; value: string }[]>(() => {
  const i: AdminInfo | null = admin.info
  if (!i) return []
  return [
    { label: '版本', value: i.version || '—' },
    { label: '主机名', value: i.hostname || '—' },
    { label: '本机 IP', value: ipv4Only(i.local_ips).join(', ') || '—' },
    { label: '监听地址', value: `${i.web_bind}:${i.web_port}` },
    { label: '门户地址', value: i.portal_base || '—' },
    { label: 'SB 地址', value: i.sb_base_url || '—' },
    { label: 'SB 用户', value: i.sb_user || '—' },
    { label: '运行时长', value: fmtUptime(i.uptime_secs) },
    { label: '用户数', value: String(i.user_count) },
    { label: '设备数', value: String(i.device_count) },
  ]
})

// --- 敏感密钥显示（SB 密码 / 云服务密钥）：默认同长度 *，点击显示 ---
const secretRevealed = ref<{ sb: boolean; rathole: boolean }>({ sb: false, rathole: false })

function toggleSecret(k: 'sb' | 'rathole') {
  secretRevealed.value = { ...secretRevealed.value, [k]: !secretRevealed.value[k] }
}

function mask(value: string): string {
  return '•'.repeat(value.length)
}

// --- 用户列表 ---
const listLoading = ref(true)
const error = ref('')
const busyId = ref('')

async function refresh() {
  listLoading.value = true
  error.value = ''
  try {
    await admin.loadUsers()
  } catch (e) {
    const msg = toErrorMessage(e)
    if (msg) error.value = msg
  } finally {
    listLoading.value = false
  }
}

// --- 密钥显示 / 复制 ---
const revealed = ref<Record<string, boolean>>({})
const copied = ref('')
let copyTimer: number | undefined

function toggleReveal(id: string) {
  revealed.value = { ...revealed.value, [id]: !revealed.value[id] }
}

async function copyKey(key: string, tag: string) {
  try {
    await navigator.clipboard.writeText(key)
    copied.value = tag
    if (copyTimer !== undefined) window.clearTimeout(copyTimer)
    copyTimer = window.setTimeout(() => {
      copied.value = ''
    }, 1500)
  } catch {
    // 剪贴板不可用（如非 HTTPS 环境）时静默忽略
  }
}

// --- 新建 / 编辑表单 ---
type DialogMode = 'create' | 'edit' | null
const dialogMode = ref<DialogMode>(null)
const editingUser = ref<PortalUser | null>(null)
const formName = ref('')
const formError = ref('')
const submitting = ref(false)

/** 表单内一行设备的草稿状态：port 为 '' 表示尚未输入；
 *  deviceName（设备名称）只读，由绑定客户端写入，提交时原样透传。 */
interface FormDevice {
  desc: string
  name: string
  port: number | ''
  pctype: string
  deviceName: string
  bound: boolean
}

const formDevices = ref<FormDevice[]>([])

// --- SB存储配置分配草稿（域名默认 fleet 域名） ---
const DEFAULT_SB_URL = 'https://md.isoops.com'
const formSbUrl = ref(DEFAULT_SB_URL)
const formSbUser = ref('')
const formSbPass = ref('')
/** SB分配密码明文显示开关（复用外部 SB 密码卡片的显示/复制交互）。 */
const sbPassVisible = ref(false)

function emptyDeviceRow(): FormDevice {
  return { desc: '', name: '', port: '', pctype: 'windows', deviceName: '', bound: false }
}

function addDeviceRow() {
  formDevices.value.push(emptyDeviceRow())
}

function removeDeviceRow(i: number) {
  formDevices.value.splice(i, 1)
}

/** 复制一份当前设备配置，插入到该行下方（服务名称需修改后才能通过保存
 * 校验）。设备名称与绑定状态不复制：两者由客户端绑定时自动写入/更新，
 * 新设备条目应为空名、未绑定。 */
function duplicateDeviceRow(i: number) {
  const { deviceName: _dn, bound: _b, ...rest } = formDevices.value[i]
  formDevices.value.splice(i + 1, 0, { ...rest, deviceName: '', bound: false })
}

/** 用户名白名单（与后端 validate_user_name 一致）。 */
const NAME_RE = /^[A-Za-z0-9_-]{1,64}$/
/** 设备服务名称：全小写英文/数字/连字符，禁止其他特殊符号。 */
const DEVICE_NAME_RE = /^[a-z0-9-]{1,64}$/

/** 设备清单展示：`办公 Mac（dev-a:4040）`。描述为空时退回服务名。 */
function fmtDevices(u: PortalUser): string {
  return u.devices.map((d) => `${d.desc || d.name}（${d.name}:${d.port}）`).join('、') || '—'
}

/** 把表单行草稿转成提交体；全空行自动忽略；设备名称（只读）原样透传。 */
function toUserDevices(): UserDevice[] {
  return formDevices.value
    .filter((d) => d.desc !== '' || d.name !== '' || d.port !== '')
    .map((d) => ({
      desc: d.desc,
      name: d.name,
      port: Number(d.port),
      'device-name': d.deviceName,
      pctype: d.pctype,
      bound: d.bound,
    }))
}

function validate(): string {
  if (!NAME_RE.test(formName.value)) {
    return '名称仅限字母、数字、下划线和连字符，长度 1-64。'
  }
  const devices = formDevices.value
  const filled = devices.filter((d) => d.desc !== '' || d.name !== '' || d.port !== '')
  if (filled.length === 0) return '请至少填写一行设备清单。'
  const seen = new Map<string, number>()
  for (let i = 0; i < devices.length; i++) {
    const d = devices[i]
    if (d.desc === '' && d.name === '' && d.port === '') continue
    if (!d.desc) {
      return `第 ${i + 1} 行描述不能为空。`
    }
    if (d.desc.length > 64) {
      return `第 ${i + 1} 行描述不能超过 64 个字符。`
    }
    if (!DEVICE_NAME_RE.test(d.name)) {
      return `第 ${i + 1} 行的服务名称「${d.name}」不合法：仅限小写字母、数字和连字符，长度 1-64。`
    }
    if (d.pctype !== 'windows' && d.pctype !== 'macos') {
      return `第 ${i + 1} 行平台类型不合法（仅支持 windows / macos）。`
    }
    const portNum = Number(d.port)
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535) {
      return `第 ${i + 1} 行（${d.desc}）的端口必须是 1-65535 的整数。`
    }
    const prev = seen.get(d.name)
    if (prev !== undefined) {
      return `设备服务名称「${d.name}」重复：第 ${prev + 1} 行与第 ${i + 1} 行，请修改后再保存。`
    }
    seen.set(d.name, i)
  }
  // SB存储配置分配：域名为必填 http(s) URL、无空白、≤200 字符
  // （账号/密码可留空，长度上限由后端校验兜底）。
  const sbUrl = formSbUrl.value
  const hasScheme = sbUrl.startsWith('http://') || sbUrl.startsWith('https://')
  if (!hasScheme || sbUrl.length > 200 || /\s/.test(sbUrl)) {
    return `SB 域名必须是以 http:// 或 https:// 开头的 URL（无空白，≤200 字符），如 ${DEFAULT_SB_URL}。`
  }
  return ''
}

function openCreate() {
  editingUser.value = null
  formName.value = ''
  formDevices.value = [emptyDeviceRow()]
  formSbUrl.value = DEFAULT_SB_URL
  formSbUser.value = ''
  formSbPass.value = ''
  sbPassVisible.value = false
  formError.value = ''
  dialogMode.value = 'create'
}

function openEdit(u: PortalUser) {
  editingUser.value = u
  formName.value = u.name
  formDevices.value = u.devices.map((d) => ({
    desc: d.desc ?? '',
    name: d.name,
    port: d.port,
    pctype: d.pctype || 'windows',
    deviceName: d['device-name'] ?? '',
    bound: d.bound ?? false,
  }))
  if (formDevices.value.length === 0) formDevices.value = [emptyDeviceRow()]
  formSbUrl.value = u.sb?.base_url || DEFAULT_SB_URL
  formSbUser.value = u.sb?.username ?? ''
  formSbPass.value = u.sb?.password ?? ''
  sbPassVisible.value = false
  formError.value = ''
  dialogMode.value = 'edit'
}

function closeForm() {
  dialogMode.value = null
  editingUser.value = null
}

async function submitForm() {
  formError.value = validate()
  if (formError.value) return
  const body: UserBody = {
    name: formName.value,
    devices: toUserDevices(),
    sb: {
      base_url: formSbUrl.value,
      username: formSbUser.value,
      password: formSbPass.value,
    },
  }
  submitting.value = true
  try {
    if (dialogMode.value === 'create') {
      const u = await admin.createUser(body)
      closeForm()
      showKeyResult(u.key, '密钥已创建')
    } else if (dialogMode.value === 'edit' && editingUser.value) {
      await admin.updateUser(editingUser.value.id, body)
      closeForm()
      await refresh()
    }
  } catch (e) {
    const msg = toErrorMessage(e)
    if (msg) formError.value = msg
  } finally {
    submitting.value = false
  }
}

// --- 密钥结果面板 ---
const resultVisible = ref(false)
const resultKey = ref('')
const resultTitle = ref('')

function showKeyResult(key: string, title: string) {
  resultKey.value = key
  resultTitle.value = title
  resultVisible.value = true
}

function closeResult() {
  resultVisible.value = false
  resultKey.value = ''
  refresh()
}

// --- 危险操作 ---
async function onRegenerate(u: PortalUser) {
  if (!window.confirm('重新生成后旧密钥将立即失效，确定？')) return
  error.value = ''
  busyId.value = u.id
  try {
    const key = await admin.regenerateKey(u.id)
    showKeyResult(key, '密钥已重新生成')
  } catch (e) {
    const msg = toErrorMessage(e)
    if (msg) error.value = msg
  } finally {
    busyId.value = ''
  }
}

async function onDelete(u: PortalUser) {
  if (!window.confirm(`删除用户「${u.name}」？该用户的登录将立即失效。`)) return
  error.value = ''
  busyId.value = u.id
  try {
    await admin.deleteUser(u.id)
    await refresh()
  } catch (e) {
    const msg = toErrorMessage(e)
    if (msg) error.value = msg
  } finally {
    busyId.value = ''
  }
}

// --- 登出 ---
async function onLogout() {
  await admin.logout()
  router.push('/admin/login')
}

onMounted(() => {
  refresh()
  admin.probe()
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
.page-head .ghost {
  min-height: 40px;
  padding: 8px 16px;
}

.section-title {
  font-size: 1.05rem;
  margin: 0 0 8px;
}
section + section .section-title {
  margin-top: 24px;
}

.info-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
}
.info-grid .card {
  margin: 0;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.info-label {
  font-size: 0.75rem;
  color: var(--text-muted);
}
.info-value {
  font-size: 0.9rem;
  word-break: break-all;
}

.secret-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 8px;
  margin-top: 8px;
}
.secret-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin: 24px 0 8px;
}
.toolbar .section-title {
  margin: 0;
}
.toolbar .primary {
  width: auto;
  min-height: 40px;
  padding: 8px 16px;
}

.empty {
  text-align: center;
}

.user-card .user-head {
  margin-bottom: 8px;
}
.uname {
  font-size: 1.05rem;
}
.user-card p {
  margin: 6px 0 0;
}
.small-text {
  font-size: 0.75rem;
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

.actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin-top: 12px;
}
.del-btn {
  color: var(--danger);
  border-color: rgba(197, 48, 48, 0.35);
}
.del-btn:hover:not(:disabled) {
  border-color: var(--danger);
}

.retry-btn {
  margin-left: 8px;
  vertical-align: middle;
}

/* 弹框遮罩 + 卡片 */
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
  max-width: 560px;
  margin: 0;
  max-height: 90vh;
  overflow-y: auto;
}
.dialog h3 {
  margin: 0 0 16px;
  font-size: 1.1rem;
}
.dialog form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.dialog label {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.lbl {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-muted);
}
.helper {
  font-size: 0.75rem;
}
.dialog-actions {
  margin-top: 4px;
}
.dialog-actions .ghost,
.dialog-actions .primary {
  flex: 1;
}
.dialog-actions .primary {
  width: auto;
}

/* 设备清单行编辑器：每台设备一个块，字段竖排（设备名称/服务名称/端口各一行），
   底部为操作行（复制/删除）；设备之间用横线分隔。 */
.devices-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-top: 1px solid var(--border);
  padding-top: 12px;
}
.device-rows {
  display: flex;
  flex-direction: column;
}
.device-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px 0;
}
/* 每台设备之间用横线分隔 */
.device-row + .device-row {
  border-top: 1px solid var(--border);
}
.device-row:first-child {
  padding-top: 0;
}
.device-row:last-child {
  padding-bottom: 0;
}
.device-row .lbl.sub {
  font-size: 0.78rem;
  font-weight: 500;
}
/* 设备名称只读（客户端绑定时写入）：禁用态沿用主题色，避免浏览器默认灰 */
.device-row input:disabled {
  color: var(--text-muted);
  background: var(--bg);
  cursor: not-allowed;
}
/* 平台类型 select：全局样式已与 input 同款（border/surface/44px/focus），
   这里仅去原生外观，箭头用 SVG 遮罩 + --text-muted 上色（自动适配深浅主题），
   视觉上与兄弟 input 完全一致。仍是原生 <select>，选项弹出由浏览器接管。 */
.pctype-field {
  position: relative;
}
.pctype-field select {
  appearance: none;
  -webkit-appearance: none;
  padding-right: 40px;
  cursor: pointer;
}
.pctype-field::after {
  content: '';
  position: absolute;
  /* label 底部为 44px 高的 select，箭头垂直居中于控件（10px 高，中心距底 22px） */
  right: 16px;
  bottom: 17px;
  width: 10px;
  height: 10px;
  pointer-events: none;
  background-color: var(--text-muted);
  -webkit-mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'%3E%3Cpath d='M1 3l4 4 4-4' fill='none' stroke='%23000' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")
    no-repeat center / contain;
  mask: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'%3E%3Cpath d='M1 3l4 4 4-4' fill='none' stroke='%23000' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")
    no-repeat center / contain;
}
/* 设备名称 + 绑定状态同行：只读输入框占满剩余宽度，药丸固定不缩 */
.bind-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.bind-row input {
  flex: 1;
  min-width: 0;
}
/* 绑定状态药丸：纯展示（无 hover/cursor），配色与 DevicesPage 卡片徽章同语义 */
.bound-pill {
  flex: none;
  font-size: 0.75rem;
  line-height: 1.5;
  padding: 2px 8px;
  border-radius: 10px;
  white-space: nowrap;
}
.bound-pill.bound-on {
  background: rgba(47, 133, 90, 0.12);
  color: var(--success);
}
.bound-pill.bound-off {
  background: var(--bg);
  color: var(--text-muted);
}
/* SB分配密码行：输入框 + 显示/复制按钮 */
.sb-pass-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.sb-pass-row input {
  flex: 1;
  min-width: 0;
}
.pass-btn {
  min-height: 44px;
  padding: 4px 12px;
}
.add-row {
  align-self: flex-start;
}
/* SB存储配置分配（域名/账号/密码） */
.sb-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
  border-top: 1px solid var(--border);
  padding-top: 12px;
}
.sb-field label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.sb-field .lbl.sub {
  font-size: 0.78rem;
  font-weight: 500;
}

/* 操作行：复制（左）+ 删除（右），与 input 同高（44px） */
.row-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
.row-act {
  flex: 1;
  min-height: 44px;
  padding: 10px 14px;
  border-radius: var(--radius);
  font-size: 0.875rem;
  background: transparent;
  border: 1px solid var(--border);
  white-space: nowrap;
  cursor: pointer;
}
.row-act.copy:hover:not(:disabled) {
  border-color: var(--text-muted);
}
.row-act.del {
  color: var(--danger);
  border-color: rgba(197, 48, 48, 0.35);
}
.row-act.del:hover:not(:disabled) {
  border-color: var(--danger);
}
.row-act:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.key-result .key-box {
  display: block;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  font-size: 1.05rem;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px;
  word-break: break-all;
  margin-bottom: 12px;
}
.key-result .primary {
  margin-top: 12px;
}
</style>
