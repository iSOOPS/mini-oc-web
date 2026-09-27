import { createRouter, createWebHistory } from 'vue-router'
import LoginPage from './pages/LoginPage.vue'
import AdminLoginPage from './pages/AdminLoginPage.vue'
import AdminPage from './pages/AdminPage.vue'
import DevicesPage from './pages/DevicesPage.vue'
import ProjectsPage from './pages/ProjectsPage.vue'
import SessionsPage from './pages/SessionsPage.vue'
import { useAdminStore, useAuthStore } from './store'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', component: LoginPage, meta: { public: true } },
    { path: '/admin/login', component: AdminLoginPage, meta: { public: true } },
    { path: '/admin', component: AdminPage, meta: { admin: true } },
    { path: '/devices', component: DevicesPage },
    {
      path: '/devices/:pctype/:pcname/projects',
      component: ProjectsPage,
    },
    {
      path: '/devices/:pctype/:pcname/sessions',
      component: SessionsPage,
    },
    { path: '/', redirect: '/devices' },
    // Device deep-link passthrough: clicking an old `/{pcname}/...` URL inside
    // the SPA must navigate there directly. We do NOT route through the SPA
    // for these — the BFF already redirects to the rathole upstream, so the
    // browser just loads it as-is.
    {
      path: '/:b64pc(^[A-Za-z0-9_-]+$)/:rest(.*)',
      component: DevicesPage,
      meta: { public: true },
    },
  ],
})

// Auth guard: three tiers.
// - meta.public (login pages + the b64pc deep-link fallback): no session
//   required. Do NOT match "public" paths by regex — an earlier version
//   also matched `/devices` and `/settings`, letting unauthenticated users
//   skip login.
// - meta.admin: probe the admin cookie via /api/admin/info; 401 → soft
//   navigate to /admin/login.
// - everything else: probe the user session via /api/me (cheap) — 401
//   means no session → soft-navigate to /login via the returned location
//   (NOT `window.location.href`, which caused a page-reload loop in v1).
router.beforeEach(async (to) => {
  if (to.meta.public) return true

  if (to.meta.admin) {
    const admin = useAdminStore()
    const ok = await admin.probe()
    if (!ok) return { path: '/admin/login', query: { redirect: to.fullPath } }
    return true
  }

  const auth = useAuthStore()
  const loggedIn = await auth.probe()
  if (!loggedIn) {
    return { path: '/login', query: { redirect: to.fullPath } }
  }
  return true
})
