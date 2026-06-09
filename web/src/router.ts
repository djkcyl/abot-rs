import { createRouter, createWebHashHistory } from 'vue-router'
import Login from './pages/Login.vue'
import Dashboard from './pages/Dashboard.vue'

const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/login', component: Login },
    { path: '/', component: Dashboard },
    { path: '/plugins', component: () => import('./pages/Plugins.vue') },
    { path: '/database', component: () => import('./pages/Database.vue') },
    { path: '/logs', component: () => import('./pages/Logs.vue') },
    { path: '/review', component: () => import('./pages/Review.vue') },
    { path: '/config', component: () => import('./pages/Config.vue') },
    { path: '/contacts', component: () => import('./pages/Groups.vue') },
    { path: '/messages', component: () => import('./pages/Messages.vue') },
    { path: '/sessions', component: () => import('./pages/Sessions.vue') },
  ],
})

export default router
