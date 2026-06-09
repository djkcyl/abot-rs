<template>
  <div style="padding: 24px">
    <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 24px">
      <h2 style="margin: 0">会话／Token 管理</h2>
      <n-button size="small" :loading="loading" @click="reload">刷新</n-button>
    </div>

    <n-alert v-if="loadError" type="error" style="margin-bottom: 16px" :title="loadError" closable @close="loadError = ''" />

    <n-data-table
      :columns="columns"
      :data="tokens"
      :loading="loading"
      size="small"
      :max-height="'calc(100vh - 200px)'"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, h, onMounted } from 'vue'
import { NDataTable, NButton, NTag, NPopconfirm, NTooltip, NAlert, useMessage } from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { send } from '../ws'

interface Token {
  token: string
  uin: number
  authority: number
  created_at: string
  expires_at: string
  valid: boolean
}

const message = useMessage()
const tokens = ref<Token[]>([])
const loading = ref(false)
const loadError = ref('')
const revoking = ref<string | null>(null)

const authorityLabel: Record<number, string> = {
  5: '主人',
  4: '超管',
  1: '已登录',
}

function maskToken(token: string): string {
  return token.length > 8 ? token.slice(0, 8) + '…' : token
}

function fmtTime(s: string): string {
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? s : d.toLocaleString()
}

const columns: DataTableColumns<Token> = [
  { title: 'QQ号', key: 'uin', width: 120 },
  {
    title: '权限',
    key: 'authority',
    width: 90,
    render: (row) => authorityLabel[row.authority] ?? String(row.authority),
  },
  {
    title: 'Token',
    key: 'token',
    render: (row) =>
      h(
        NTooltip,
        {},
        {
          trigger: () => h('span', { style: 'font-family: monospace' }, maskToken(row.token)),
          default: () => '完整 token 不在此明文展示',
        },
      ),
  },
  { title: '签发时间', key: 'created_at', render: (row) => fmtTime(row.created_at) },
  { title: '过期时间', key: 'expires_at', render: (row) => fmtTime(row.expires_at) },
  {
    title: '状态',
    key: 'valid',
    width: 90,
    render: (row) =>
      h(NTag, { type: row.valid ? 'success' : 'default', size: 'small' }, { default: () => (row.valid ? '有效' : '已过期') }),
  },
  {
    title: '操作',
    key: '__actions__',
    width: 90,
    render: (row) =>
      h(
        NPopconfirm,
        { positiveText: '确定', negativeText: '取消', onPositiveClick: () => revoke(row) },
        {
          trigger: () =>
            h(NButton, { size: 'tiny', type: 'error', loading: revoking.value === row.token }, { default: () => '吊销' }),
          default: () => '确定吊销该会话？',
        },
      ),
  },
]

async function reload() {
  loading.value = true
  loadError.value = ''
  try {
    const resp = (await send('tokens/list', {})) as { tokens: Token[] }
    tokens.value = resp.tokens ?? []
  } catch (e) {
    loadError.value = typeof e === 'string' ? e : String(e)
  } finally {
    loading.value = false
  }
}

async function revoke(row: Token) {
  revoking.value = row.token
  try {
    await send('token/revoke', { token: row.token })
    message.success('已吊销')
    await reload()
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    revoking.value = null
  }
}

onMounted(reload)
</script>
