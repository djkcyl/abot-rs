<template>
  <div style="padding: 24px; display: flex; flex-direction: column; gap: 12px; height: calc(100vh - 48px); box-sizing: border-box">
    <!-- 控制栏 -->
    <n-card content-style="padding: 12px 16px">
      <div style="display: flex; align-items: center; gap: 16px; flex-wrap: wrap">
        <n-checkbox-group v-model:value="activeLevels">
          <n-space>
            <n-checkbox value="error" label="错误" />
            <n-checkbox value="warn" label="警告" />
            <n-checkbox value="info" label="信息" />
            <n-checkbox value="debug" label="调试" />
          </n-space>
        </n-checkbox-group>
        <n-input
          v-model:value="keyword"
          placeholder="关键词过滤（来源／正文）"
          clearable
          size="small"
          style="width: 220px"
        />
        <n-switch v-model:value="paused" size="small">
          <template #checked>已暂停</template>
          <template #unchecked>滚动中</template>
        </n-switch>
        <n-button size="small" @click="clearView">清屏</n-button>
        <span style="font-size: 12px; color: #888; margin-left: auto">
          共 {{ visibleLines.length }} 行
        </span>
      </div>
    </n-card>

    <!-- 日志视图 -->
    <n-card style="flex: 1; overflow: hidden" content-style="padding: 0; height: 100%">
      <div
        ref="scrollEl"
        style="height: 100%; overflow: auto; padding: 8px 12px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.6"
        @scroll="onScroll"
      >
        <div v-if="visibleLines.length === 0" style="color: #666; padding: 16px">
          暂无日志。
        </div>
        <div
          v-for="(line, idx) in visibleLines"
          :key="idx"
          style="white-space: pre-wrap; word-break: break-all"
          :style="{ color: levelColor(line.level) }"
        >
          <span style="color: #666">{{ fmtTime(line.ts) }}</span>
          <span :style="{ color: levelColor(line.level), fontWeight: 600 }"> {{ levelLabel(line.level) }} </span>
          <span style="color: #888">{{ line.target }}</span>
          <span style="color: #aaa"> － </span>
          <span>{{ line.msg }}</span>
        </div>
      </div>
    </n-card>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted } from 'vue'
import {
  NCard,
  NCheckbox,
  NCheckboxGroup,
  NSpace,
  NInput,
  NSwitch,
  NButton,
} from 'naive-ui'
import { store } from '../ws'

interface LogLine {
  ts: number
  level: string
  target: string
  msg: string
}

// 后端 level 为大写（INFO/WARN/…），统一按小写比较。
function norm(level: string): string {
  return (level || '').toLowerCase()
}

const allLines = computed<LogLine[]>(() => (store['logs'] as LogLine[] | undefined) ?? [])

const activeLevels = ref<string[]>(['error', 'warn', 'info', 'debug'])
const keyword = ref('')
const paused = ref(false)

// 清屏只清本地视图：记下当前已收到的行数作为下界，之后只显示其后的行。
const clearedBefore = ref(0)

const visibleLines = computed<LogLine[]>(() => {
  const kw = keyword.value.trim().toLowerCase()
  const levels = new Set(activeLevels.value)
  return allLines.value.slice(clearedBefore.value).filter((line) => {
    if (!levels.has(norm(line.level))) return false
    if (kw && !`${line.target} ${line.msg}`.toLowerCase().includes(kw)) return false
    return true
  })
})

const scrollEl = ref<HTMLElement | null>(null)

function scrollToBottom() {
  const el = scrollEl.value
  if (el) el.scrollTop = el.scrollHeight
}

// 用户手动上滚时自动暂停；滚回底部则恢复。
function onScroll() {
  const el = scrollEl.value
  if (!el) return
  const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24
  if (!atBottom && !paused.value) {
    paused.value = true
  }
}

function clearView() {
  clearedBefore.value = allLines.value.length
}

function levelColor(level: string): string {
  switch (norm(level)) {
    case 'error':
      return '#f5222d'
    case 'warn':
      return '#faad14'
    case 'debug':
      return '#888'
    default:
      return '#d9d9d9'
  }
}

function levelLabel(level: string): string {
  return norm(level).toUpperCase().padEnd(5, ' ')
}

function fmtTime(ts: number): string {
  const d = new Date(ts)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}.${p(d.getMilliseconds(), 3)}`
}

// 新行到来且未暂停时滚到底。
watch(
  () => allLines.value.length,
  () => {
    if (!paused.value) nextTick(scrollToBottom)
  },
)

// 取消暂停时立即滚到底。
watch(paused, (v) => {
  if (!v) nextTick(scrollToBottom)
})

onMounted(() => {
  nextTick(scrollToBottom)
})
</script>
