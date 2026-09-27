<template>
  <div class="login-wrap">
    <div class="login-card">
      <AppLogo class="logo" :size="72" />
      <h1 class="title">mini-oc-web</h1>
      <p class="subtitle">跨设备 opencode 会话门户</p>
      <form @submit.prevent="onSubmit">
        <label>
          <span class="lbl">登录密钥</span>
          <input
            v-model.trim="key"
            type="text"
            class="key-input"
            placeholder="32 位密钥"
            autocomplete="current-password"
            maxlength="64"
            required
            autofocus
          />
        </label>
        <p v-if="error" class="error">{{ error }}</p>
        <button class="primary" :disabled="auth.loading">
          {{ auth.loading ? '登录中…' : '登录' }}
        </button>
      </form>
      <p class="muted hint">密钥由管理员在管理后台为你创建。</p>
      <p class="muted hint">
        <router-link to="/admin/login">管理入口 →</router-link>
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '../store'
import AppLogo from '../components/AppLogo.vue'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()

const key = ref('')
const error = ref('')

async function onSubmit() {
  error.value = ''
  try {
    await auth.login(key.value)
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : ''
    router.push(redirect && redirect.startsWith('/') ? redirect : '/devices')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}
</script>

<style scoped>
.login-wrap {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 100vh;
  padding: 24px 16px;
}
.login-card {
  width: 100%;
  max-width: 400px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 28px 22px;
  box-shadow: var(--shadow);
}
.logo {
  margin: 0 auto 16px;
}
.title {
  margin: 0 0 4px;
  font-size: 1.5rem;
  text-align: center;
}
.subtitle {
  margin: 0 0 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: 0.9rem;
}
form {
  display: flex;
  flex-direction: column;
  gap: 14px;
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
.key-input {
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  letter-spacing: 0.05em;
}
.hint {
  margin-top: 16px;
  text-align: center;
  font-size: 0.8rem;
}
</style>
