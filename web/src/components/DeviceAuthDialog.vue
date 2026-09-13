<template>
  <div v-if="visible" class="auth-overlay">
    <div class="auth-card" role="dialog" aria-modal="true" aria-label="设备授权">
      <h2 class="title">设备授权</h2>
      <p class="subtitle">
        该设备需要单独的账号密码才能访问。<template v-if="baseUrl">
          设备地址：<code>{{ baseUrl }}</code></template>
      </p>
      <form @submit.prevent="onSubmit">
        <label>
          <span class="lbl">账号</span>
          <input
            ref="userInput"
            v-model.trim="username"
            type="text"
            autocomplete="username"
            required
          />
        </label>
        <label>
          <span class="lbl">密码</span>
          <input v-model="password" type="password" autocomplete="current-password" required />
        </label>
        <p v-if="error" class="error">{{ error }}</p>
        <div class="actions">
          <button type="button" class="ghost" :disabled="busy" @click="onCancel">取消</button>
          <button type="submit" class="primary" :disabled="busy">
            {{ busy ? '验证中…' : '授权' }}
          </button>
        </div>
      </form>
      <p class="muted hint">
        账密即该设备 TUI 配置的 OPENCODE_SERVER_USERNAME/PASSWORD。授权仅在本次
        BFF 运行期间有效，不会持久保存。
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'

const props = defineProps<{
  visible: boolean
  baseUrl?: string
  busy: boolean
  error: string
}>()

const emit = defineEmits<{
  submit: [username: string, password: string]
  cancel: []
}>()

const username = ref('')
const password = ref('')
const userInput = ref<HTMLInputElement | null>(null)

watch(
  () => props.visible,
  async (v) => {
    if (v) {
      username.value = ''
      password.value = ''
      await nextTick()
      userInput.value?.focus()
    }
  },
)

function onSubmit() {
  if (!username.value || !password.value) return
  emit('submit', username.value, password.value)
}

function onCancel() {
  if (props.busy) return
  emit('cancel')
}
</script>

<style scoped>
.auth-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px 16px;
  z-index: 100;
}
.auth-card {
  width: 100%;
  max-width: 400px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px 20px;
  box-shadow: var(--shadow);
}
.title {
  margin: 0 0 4px;
  font-size: 1.2rem;
}
.subtitle {
  margin: 0 0 18px;
  color: var(--text-muted);
  font-size: 0.85rem;
  word-break: break-all;
}
.subtitle code {
  font-size: 0.8rem;
}
form {
  display: flex;
  flex-direction: column;
  gap: 12px;
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
.actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 4px;
}
.hint {
  margin: 14px 0 0;
  text-align: center;
  font-size: 0.78rem;
}
</style>
