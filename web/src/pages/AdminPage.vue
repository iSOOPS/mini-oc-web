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
          <section class="form-card">
            <span class="form-card-title">名称</span>
            <label>
              <input
                v-model.trim="formName"
                type="text"
                placeholder="如 alice"
                maxlength="64"
                autofocus
              />
            </label>
          </section>
          <!-- SB存储配置分配（域名/账号/密码，随用户存储到远程注册表） -->
          <section class="form-card">
            <span class="form-card-title">SB存储配置分配</span>
            <label>
              <span class="lbl sub">域名</span>
              <input
                v-model.trim="formSbUrl"
                type="text"
                maxlength="200"
                placeholder="https://sb.example.com"
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
              分配给该用户的 SB 存储配置（域名默认 https://sb.example.com）；用户登录后可在设置中查看并按需修改。
            </span>
          </section>

          <section class="form-card">
            <span class="form-card-title">设备清单</span>
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
                  <span class="lbl sub">服务端口号</span>
                  <input
                    v-model.number="d.port"
                    type="number"
                    placeholder="本地 TUI 服务端口，如 9465"
                    min="1"
                    max="65535"
                  />
                </label>
                <label>
                  <span class="lbl sub">OpenCode 端口号</span>
                  <input
                    v-model.number="d.ocPort"
                    type="number"
                    placeholder="可空；填写后以 /端口号 路径前缀拼接跳转地址"
                    min="1"
                    max="65535"
                  />
                </label>
                <label class="public-url-field">
                  <span class="lbl sub">设备穿透地址</span>
                  <div class="tunnel-row">
                    <select v-model="d.publicScheme" class="scheme-select" aria-label="协议">
                      <option value="http">http</option>
                      <option value="https">https</option>
                    </select>
                    <input
                      v-model.trim="d.publicHost"
                      type="text"
                      placeholder="IP 或域名，可粘贴含协议完整地址"
                      maxlength="200"
                      autocomplete="off"
                    />
                  </div>
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
              每台设备包含：描述（展示用，1-64 字符）、服务名称（仅限小写字母、数字、连字符，不可重复）、平台类型（windows/macos）、服务端口号（本地 TUI 服务端口，1-65535；穿透地址不带端口时，BFF 以 穿透地址+服务端口号 探活/调 TUI 接口）、OpenCode 端口号（可空；非空时跳转 oc web 以 穿透地址/OpenCode端口号 路径前缀拼接，为空时跳转地址即穿透地址本身）、设备穿透地址（IP 或域名，可自带端口如 1.2.3.4:9000，支持 IPv6 与下划线/中文等非常规域名；可直接粘贴含协议的完整地址，保存时自动去除协议与路径；地址自带端口时 BFF 直接以该地址探活）、设备名称（只读）与绑定状态（均由客户端绑定时自动写入/更新，不可人工修改）；「复制」会克隆一份当前设备配置，保存时服务名称不可与其他设备重复。
            </span>
          </section>
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
  ]
})

// --- 敏感密钥显示（SB 密码）：默认同长度 *，点击显示 ---
const secretRevealed = ref<{ sb: boolean }>({ sb: false })

function toggleSecret(k: 'sb') {
  secretRevealed.value = { ...secretRevealed.value, [k]: !secretRevealed.value[k] }
}

function mask(value: string | undefined): string {
  // 后端字段缺失/为空时兜底为空串，避免 undefined.length 崩溃渲染
  return '•'.repeat(value?.length ?? 0)
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

/** 表单内一行设备的草稿状态：port / ocPort 为 '' 表示尚未输入
 *  （ocPort 允许留空 = 跳转地址不含 /oc-port 段）；deviceName（设备
 *  名称）只读，由绑定客户端写入，提交时原样透传；publicScheme +
 *  publicHost 组成设备穿透地址（host 段可自带端口）。 */
interface FormDevice {
  desc: string
  name: string
  port: number | ''
  ocPort: number | ''
  pctype: string
  publicScheme: string
  publicHost: string
  deviceName: string
  bound: boolean
}

const formDevices = ref<FormDevice[]>([])

// --- SB存储配置分配草稿（域名默认 fleet 域名） ---
const DEFAULT_SB_URL = 'http://127.0.0.1:3000'
const formSbUrl = ref(DEFAULT_SB_URL)
const formSbUser = ref('')
const formSbPass = ref('')
/** SB分配密码明文显示开关（复用外部 SB 密码卡片的显示/复制交互）。 */
const sbPassVisible = ref(false)

function emptyDeviceRow(): FormDevice {
  return {
    desc: '',
    name: '',
    port: '',
    ocPort: '',
    pctype: 'windows',
    publicScheme: 'https',
    publicHost: '',
    deviceName: '',
    bound: false,
  }
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

/** 设备穿透地址主机部分：宽松校验 —— 允许 `host`、`host:port`、
 *  `[IPv6]`、`[IPv6]:port`。host 段只拒绝会破坏 URL 拼接的字符
 *  （空白、路径/查询/锚点起始符、userinfo、方括号、WHATWG 禁用符号、
 *  控制字符）；域名（含下划线、非 ASCII 等非常规形态）均放行，与后端
 *  `validate_user_devices` 的实际契约对齐。端口段须为 1-65535。 */
const HOST_FORBIDDEN_RE = /[\s:\/?#\\@[\]<>^|%\u0000-\u001f\u007f]/
const IPV6_LITERAL_RE = /^\[[0-9A-Fa-f:.]+\]$/
const PORT_RE = /^[0-9]{1,5}$/

function validPortSuffix(s: string): boolean {
  if (!PORT_RE.test(s)) return false
  const n = Number(s)
  return n >= 1 && n <= 65535
}

function validTunnelHost(s: string): boolean {
  if (s === '') return false
  if (s.startsWith('[')) {
    const close = s.indexOf(']')
    if (close === -1) return false
    if (!IPV6_LITERAL_RE.test(s.slice(0, close + 1))) return false
    const tail = s.slice(close + 1)
    return tail === '' || (tail.startsWith(':') && validPortSuffix(tail.slice(1)))
  }
  const colon = s.lastIndexOf(':')
  if (colon === -1) return !HOST_FORBIDDEN_RE.test(s)
  const host = s.slice(0, colon)
  const port = s.slice(colon + 1)
  return host !== '' && !HOST_FORBIDDEN_RE.test(host) && validPortSuffix(port)
}

/** 粘贴归一化：用户常把穿透服务商给的完整地址粘进主机框，这里剥掉
 *  协议前缀（并入协议下拉）与尾部路径/斜杠，其余（含 `host:port`）
 *  原样保留 —— 地址自带端口是合法形态。 */
function peelTunnelInput(d: FormDevice): void {
  let s = d.publicHost.trim()
  const scheme = /^(https?):\/\//i.exec(s)
  if (scheme) {
    d.publicScheme = scheme[1].toLowerCase()
    s = s.slice(scheme[0].length)
  }
  const cut = s.search(/[/?#]/)
  if (cut >= 0) s = s.slice(0, cut)
  d.publicHost = s.trim()
}

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
      'oc-port': d.ocPort === '' || d.ocPort === null ? null : Number(d.ocPort),
      'device-name': d.deviceName,
      pctype: d.pctype,
      bound: d.bound,
      'public-url': `${d.publicScheme}://${d.publicHost}`,
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
    peelTunnelInput(d)
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
      return `第 ${i + 1} 行（${d.desc}）的服务端口号必须是 1-65535 的整数。`
    }
    const ocPortRaw = d.ocPort === null || d.ocPort === undefined ? '' : String(d.ocPort)
    if (ocPortRaw !== '') {
      const ocPortNum = Number(ocPortRaw)
      if (!Number.isInteger(ocPortNum) || ocPortNum < 1 || ocPortNum > 65535) {
        return `第 ${i + 1} 行（${d.desc}）的 OpenCode 端口号必须是 1-65535 的整数（留空表示跳转地址不含该段）。`
      }
      if (ocPortNum === portNum) {
        return `第 ${i + 1} 行（${d.desc}）的 OpenCode 端口号须与服务端口号不同（设备端 TUI 拒绝相同端口）。`
      }
    }
    if (!validTunnelHost(d.publicHost)) {
      return `第 ${i + 1} 行（${d.desc}）的设备穿透地址「${d.publicHost}」不合法：请填写 IP 或域名，可带端口（如 1.2.3.4:9000），也可直接粘贴含协议的完整地址（自动去除协议与路径）。`
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

/** 把存储的 public-url（scheme://host[:port]）拆成表单的 scheme +
 *  host；host 段可自带端口。IPv6 字面量含冒号，按方括号整体捕获，
 *  其后可跟 :port。 */
function splitPublicUrl(url: string): { scheme: string; host: string } {
  const m = /^(https?):\/\/(\[[0-9A-Fa-f:.]+\](?::\d{1,5})?|[^/?#@]+)/i.exec(url.trim())
  if (!m) return { scheme: 'https', host: '' }
  return { scheme: m[1].toLowerCase(), host: m[2] }
}

function openEdit(u: PortalUser) {
  editingUser.value = u
  formName.value = u.name
  formDevices.value = u.devices.map((d) => {
    const { scheme, host } = splitPublicUrl(d['public-url'] ?? '')
    return {
      desc: d.desc ?? '',
      name: d.name,
      port: d.port,
      ocPort: d['oc-port'] ?? '',
      pctype: d.pctype || 'windows',
      publicScheme: scheme,
      publicHost: host,
      deviceName: d['device-name'] ?? '',
      bound: d.bound ?? false,
    }
  })
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

/* 表单分组卡片：浅色底 + 边框，包裹 名称 / SB存储配置分配 / 设备清单 */
.form-card {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.form-card-title {
  font-size: 0.9rem;
  font-weight: 700;
  color: var(--text);
}
.form-card label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.form-card .lbl.sub {
  font-size: 0.78rem;
  font-weight: 500;
}

/* 设备清单行编辑器：每台设备一个浅色卡片，字段竖排，
   底部为操作行（复制/删除）。 */
.device-rows {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.device-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px;
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
/* 设备穿透地址：协议下拉与主机输入分行堆叠、各自全宽 —— 移动端一行
   放不下两个控件，且与表单其余字段（服务名称/端口号等）的竖排全宽
   风格保持一致。 */
.tunnel-row {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
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
