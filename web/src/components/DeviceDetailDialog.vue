<template>
  <div v-if="visible" class="dd-overlay">
    <div class="dd-card" role="dialog" aria-modal="true" aria-label="设备详情">
      <h2 class="title">设备详情</h2>
      <p v-if="loading" class="muted">加载中…</p>
      <template v-else-if="detail">
        <dl class="fields">
          <div class="field">
            <dt>名称</dt>
            <dd>{{ detail.pcname }}</dd>
          </div>
          <div class="field">
            <dt>类型</dt>
            <dd>{{ detail.pctype }}</dd>
          </div>
          <div class="field">
            <dt>地址</dt>
            <dd class="break">{{ detail.public_url }}</dd>
          </div>
          <div class="field">
            <dt>连接账号</dt>
            <dd class="break">{{ detail.username }}</dd>
          </div>
          <div class="field">
            <dt>连接密码</dt>
            <dd
              v-if="detail.password"
              class="break mono clickable"
              :title="revealed ? '点击隐藏' : '点击显示原文'"
              @click="revealed = !revealed"
            >
              {{ revealed ? detail.password : mask }}
            </dd>
            <dd v-else class="muted-dd">（未配置）</dd>
          </div>
        </dl>
        <p class="muted hint">密码默认隐藏为等长 * 号，点击可显示 / 隐藏原文。</p>
      </template>
      <p v-else-if="error" class="error">{{ error }}</p>
      <div class="actions">
        <button type="button" class="primary" @click="onClose">关闭</button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { apiGet, UnauthorizedError } from '../api'

export interface DeviceDetail {
  pctype: string
  pcname: string
  public_url: string
  oc_serve_port?: number
  version?: string | null
  username: string
  password: string
}

const props = defineProps<{
  visible: boolean
  /** `/api/devices/:pctype/:pcname` path of the target device. */
  devicePath: string
}>()

const emit = defineEmits<{ close: [] }>()

const detail = ref<DeviceDetail | null>(null)
const loading = ref(false)
const error = ref('')
const revealed = ref(false)

/** 与真实密码等长的 * 号（密码较长时靠 break-all 自动换行）。 */
const mask = computed(() => '*'.repeat(detail.value?.password.length ?? 0))

watch(
  () => props.visible,
  async (v) => {
    if (!v) return
    detail.value = null
    revealed.value = false
    error.value = ''
    loading.value = true
    try {
      detail.value = await apiGet<DeviceDetail>(`${props.devicePath}/detail`)
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        emit('close')
        return
      }
      error.value = e instanceof Error ? e.message : String(e)
    } finally {
      loading.value = false
    }
  },
)

function onClose() {
  emit('close')
}
</script>

<style scoped>
.dd-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px 16px;
  z-index: 100;
}
.dd-card {
  width: 100%;
  max-width: 440px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px 20px;
  box-shadow: var(--shadow);
  max-height: 85vh;
  overflow-y: auto;
}
.title {
  margin: 0 0 16px;
  font-size: 1.2rem;
}
.fields {
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.field dt {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.field dd {
  margin: 0;
  font-size: 0.95rem;
}
.break {
  word-break: break-all;
  white-space: pre-wrap;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
}
.clickable {
  cursor: pointer;
  user-select: none;
  border: 1px dashed var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
  line-height: 1.6;
}
.clickable:hover {
  border-color: var(--primary);
}
.muted-dd {
  color: var(--text-muted);
}
.hint {
  margin: 14px 0 0;
  font-size: 0.78rem;
  text-align: center;
}
.actions {
  margin-top: 16px;
}
</style>
