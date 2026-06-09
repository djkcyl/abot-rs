<template>
  <div style="display: flex; justify-content: center; align-items: center; height: 100vh; background: #18181c">
    <n-card style="width: 420px" title="abot 控制台登录">
      <div v-if="!code">
        <p style="margin-bottom: 16px; color: #aaa">点「获取验证码」，私聊把验证码发给机器人。</p>
        <n-button
          type="primary"
          block
          :loading="loading"
          @click="getCode"
        >
          获取验证码
        </n-button>
        <n-alert v-if="errorMsg" type="error" style="margin-top: 12px">{{ errorMsg }}</n-alert>
      </div>
      <div v-else>
        <p style="margin-bottom: 8px; color: #aaa">请私聊机器人发送:</p>
        <div style="text-align: center; margin: 16px 0">
          <n-text style="font-size: 36px; font-weight: bold; letter-spacing: 8px; color: #63e2b7">
            {{ code }}
          </n-text>
        </div>
        <n-alert type="info" style="margin-bottom: 16px">
          {{ hint || `登录 ${code}` }}
        </n-alert>
        <div style="text-align: center; margin-bottom: 12px">
          <n-spin v-if="polling" size="small" />
          <span style="color: #aaa; margin-left: 8px; font-size: 13px">
            {{ polling ? '等待确认中…' : '已过期，请重新获取' }}
          </span>
        </div>
        <n-progress
          v-if="polling"
          type="line"
          :percentage="progressPct"
          :indicator-placement="'inside'"
          style="margin-bottom: 12px"
        />
        <n-button text block @click="reset" style="color: #aaa">重新获取验证码</n-button>
        <n-alert v-if="errorMsg" type="error" style="margin-top: 12px">{{ errorMsg }}</n-alert>
      </div>
    </n-card>
  </div>
</template>

<script setup lang="ts">
import { ref, onUnmounted } from 'vue'
import {
  NCard,
  NButton,
  NAlert,
  NSpin,
  NText,
  NProgress,
} from 'naive-ui'
import { challenge, poll, setToken } from '../auth'

const emit = defineEmits<{
  (e: 'logged-in', token: string): void
}>()

const code = ref('')
const hint = ref('')
const loading = ref(false)
const polling = ref(false)
const errorMsg = ref('')

// 5 分钟 = 300 秒超时
const POLL_TIMEOUT_MS = 5 * 60 * 1000
const POLL_INTERVAL_MS = 2000
const progressPct = ref(100)

let pollTimer: ReturnType<typeof setInterval> | null = null
let timeoutTimer: ReturnType<typeof setTimeout> | null = null
let elapsed = 0

function clearPolling() {
  if (pollTimer !== null) { clearInterval(pollTimer); pollTimer = null }
  if (timeoutTimer !== null) { clearTimeout(timeoutTimer); timeoutTimer = null }
  polling.value = false
}

async function getCode() {
  loading.value = true
  errorMsg.value = ''
  try {
    const result = await challenge()
    code.value = result.code
    hint.value = result.hint
    startPolling()
  } catch (e) {
    errorMsg.value = e instanceof Error ? e.message : '获取验证码失败'
  } finally {
    loading.value = false
  }
}

function startPolling() {
  polling.value = true
  elapsed = 0
  progressPct.value = 100

  pollTimer = setInterval(async () => {
    elapsed += POLL_INTERVAL_MS
    progressPct.value = Math.max(0, 100 - (elapsed / POLL_TIMEOUT_MS) * 100)
    try {
      const result = await poll(code.value)
      if (result.token) {
        clearPolling()
        setToken(result.token, result.authority ?? 1)
        emit('logged-in', result.token)
      }
    } catch {
      // 忽略单次 poll 失败,继续轮询
    }
  }, POLL_INTERVAL_MS)

  timeoutTimer = setTimeout(() => {
    clearPolling()
    errorMsg.value = '验证码已过期，请重新获取。'
    code.value = ''
  }, POLL_TIMEOUT_MS)
}

function reset() {
  clearPolling()
  code.value = ''
  hint.value = ''
  errorMsg.value = ''
}

onUnmounted(() => { clearPolling() })
</script>
