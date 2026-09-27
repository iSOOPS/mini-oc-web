<template>
  <div class="login-wrap">
    <div class="login-card">
      <AppLogo class="logo" :size="56" />
      <h1 class="title">管理后台</h1>
      <p class="subtitle">mini-oc-web 多租户管理</p>
      <form @submit.prevent="onSubmit">
        <label>
          <span class="lbl">SB 密码</span>
          <input
            v-model="password"
            type="password"
            placeholder="SB_PASSWORD"
            autocomplete="current-password"
            required
            autofocus
          />
        </label>
        <p v-if="error" class="error">{{ error }}</p>
        <button class="primary" :disabled="admin.loading">
          {{ admin.loading ? '登录中…' : '登录' }}
        </button>
      </form>
      <p class="muted hint">管理员密码与服务端 SB_PASSWORD 环境变量一致。</p>
      <p class="muted hint">
        <router-link to="/login">← 返回用户登录</router-link>
      </p>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAdminStore } from '../store'
import AppLogo from '../components/AppLogo.vue'

const admin = useAdminStore()
const router = useRouter()
const route = useRoute()

const password = ref('')
const error = ref('')

async function onSubmit() {
  error.value = ''
  try {
    await admin.login(password.value)
    const redirect = typeof route.query.redirect === 'string' ? route.query.redirect : ''
    router.push(redirect.startsWith('/') ? redirect : '/admin')
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
.hint {
  margin-top: 16px;
  text-align: center;
  font-size: 0.8rem;
}
.hint + .hint {
  margin-top: 4px;
}
</style>
