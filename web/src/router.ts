import { createRouter, createWebHistory } from 'vue-router'
import LoginPage from './pages/LoginPage.vue'
import DevicesPage from './pages/DevicesPage.vue'
import ProjectsPage from './pages/ProjectsPage.vue'
import SessionsPage from './pages/SessionsPage.vue'
import SettingsPage from './pages/SettingsPage.vue'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', component: LoginPage },
    { path: '/devices', component: DevicesPage },
    { path: '/devices/:pctype/:pcname/projects', component: ProjectsPage },
    { path: '/devices/:pctype/:pcname/sessions', component: SessionsPage },
    { path: '/settings', component: SettingsPage },
    { path: '/', redirect: '/devices' },
  ],
})
