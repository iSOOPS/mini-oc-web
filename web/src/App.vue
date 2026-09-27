<template>
  <div class="layout">
    <header v-if="auth.me" class="topbar">
      <div class="topbar-title">
        <router-link to="/devices" class="brand">
          <AppLogo :size="22" />
          <span>mini-oc-web</span>
        </router-link>
      </div>
      <div class="topbar-actions">
        <button class="ghost topbar-btn" @click="settingsVisible = true">
          {{ auth.me.name }}
        </button>
        <button class="ghost topbar-btn" @click="onLogout">登出</button>
      </div>
    </header>
    <main class="main">
      <router-view />
    </main>
    <UserSettingsDialog :visible="settingsVisible" @close="settingsVisible = false" />
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from './store'
import UserSettingsDialog from './components/UserSettingsDialog.vue'
import AppLogo from './components/AppLogo.vue'

const auth = useAuthStore()
const router = useRouter()
const settingsVisible = ref(false)

async function onLogout() {
  await auth.logout()
  router.push('/login')
}
</script>

<style scoped>
.layout {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
}
.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  z-index: 10;
}
.topbar-title {
  font-weight: 700;
  font-size: 1.05rem;
}
.topbar-title a {
  color: var(--text);
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
}
.topbar-actions {
  display: flex;
  gap: 12px;
  align-items: center;
}
.topbar-user {
  color: var(--text-muted);
  font-size: 0.9rem;
}
/* 顶栏按钮统一 ghost 风格（用户名 / 登出）。 */
.topbar-btn {
  min-height: 36px;
  padding: 6px 14px;
  font-size: 0.9rem;
}
.topbar-link {
  font-size: 0.95rem;
  padding: 8px 12px;
  border-radius: 6px;
}
.link-btn {
  background: transparent;
  border: 0;
  color: var(--primary);
  font-size: 0.95rem;
  padding: 8px 12px;
  min-height: 44px;
}
.main {
  flex: 1;
  width: 100%;
  max-width: 720px;
  margin: 0 auto;
  padding: 16px;
}
</style>