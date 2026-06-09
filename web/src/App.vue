<template>
  <n-config-provider :theme="darkTheme">
    <n-message-provider>
      <n-layout v-if="!token" style="height: 100vh">
        <Login @logged-in="onLoggedIn" />
      </n-layout>
      <n-layout v-else has-sider style="height: 100vh">
        <n-layout-sider
          bordered
          collapse-mode="width"
          :collapsed-width="64"
          :width="200"
          :collapsed="collapsed"
          show-trigger="bar"
          @collapse="collapsed = true"
          @expand="collapsed = false"
        >
          <div style="padding: 16px 0; text-align: center; font-weight: bold; font-size: 16px">
            <span v-if="!collapsed">abot</span>
            <span v-else>A</span>
          </div>
          <n-menu
            :collapsed="collapsed"
            :collapsed-width="64"
            :collapsed-icon-size="22"
            :options="menuOptions"
            :value="currentRoute"
            @update:value="handleMenuSelect"
          />
          <div style="position: absolute; bottom: 16px; width: 100%; text-align: center">
            <n-button text @click="logout">
              {{ collapsed ? '⇐' : '退出登录' }}
            </n-button>
          </div>
        </n-layout-sider>
        <n-layout-content style="overflow: auto">
          <router-view />
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider,
  NLayout,
  NLayoutSider,
  NLayoutContent,
  NMenu,
  NButton,
  NMessageProvider,
  darkTheme,
  type MenuOption,
} from 'naive-ui'
import Login from './pages/Login.vue'
import { getToken, getAuthority, clearToken } from './auth'
import { connect, disconnect } from './ws'

const router = useRouter()
const route = useRoute()

const token = ref<string | null>(getToken())
const collapsed = ref(false)

const authority = computed(() => getAuthority())

const currentRoute = computed(() => route.path)

const menuOptions = computed<MenuOption[]>(() => [
  {
    label: '总览',
    key: '/',
  },
  {
    label: '插件',
    key: '/plugins',
    disabled: false,
  },
  {
    label: '群管理',
    key: '/contacts',
    disabled: authority.value < 4,
  },
  {
    label: '消息',
    key: '/messages',
    disabled: authority.value < 4,
  },
  {
    label: '审核',
    key: '/review',
    disabled: authority.value < 4,
  },
  {
    label: '配置',
    key: '/config',
    disabled: authority.value < 4,
  },
  {
    label: '数据库',
    key: '/database',
    disabled: authority.value < 4,
  },
  {
    label: '会话',
    key: '/sessions',
    disabled: authority.value < 5,
  },
  {
    label: '日志',
    key: '/logs',
    disabled: authority.value < 4,
  },
])

function handleMenuSelect(key: string) {
  if (key === route.path) return
  router.push(key)
}

function onLoggedIn(newToken: string) {
  token.value = newToken
  connect(newToken)
  router.push('/')
}

function logout() {
  disconnect()
  clearToken()
  token.value = null
  router.push('/login')
}

// 启动时如果已有 token 则连接
if (token.value) {
  connect(token.value)
}

watch(token, (val) => {
  if (!val) {
    disconnect()
  }
})

// 没有 token 时重定向到登录
watch(
  () => route.path,
  (path) => {
    if (!token.value && path !== '/login') {
      router.push('/login')
    }
  },
  { immediate: true },
)
</script>
