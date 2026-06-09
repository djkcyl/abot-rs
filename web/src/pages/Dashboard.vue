<template>
  <div style="padding: 24px">
    <div style="display: flex; align-items: center; justify-content: space-between; margin-bottom: 24px">
      <h2 style="margin: 0">总览</h2>
      <n-button size="small" :loading="loading" @click="load">刷新</n-button>
    </div>

    <n-alert v-if="loadError" type="error" style="margin-bottom: 16px" :title="loadError" closable @close="loadError = ''" />

    <!-- 身份卡 -->
    <n-card style="margin-bottom: 16px" content-style="padding: 20px">
      <n-spin :show="loading && !bot">
        <div style="display: flex; align-items: center; gap: 20px; flex-wrap: wrap">
          <n-avatar
            :size="72"
            round
            :src="bot?.avatar"
            :img-props="{ referrerpolicy: 'no-referrer' }"
          />
          <div style="min-width: 200px">
            <div style="display: flex; align-items: center; gap: 12px; flex-wrap: wrap">
              <span style="font-size: 22px; font-weight: 600">{{ bot?.nickname || '未知昵称' }}</span>
              <n-tag :type="bot?.online ? 'success' : 'error'" size="small" round>
                {{ bot?.online ? '在线' : '离线' }}
              </n-tag>
            </div>
            <div style="margin-top: 4px; color: #888; font-size: 13px">
              QQ {{ bot?.uin ?? '—' }}
            </div>
          </div>

          <!-- 右侧元信息 -->
          <div style="margin-left: auto; display: flex; gap: 32px; flex-wrap: wrap">
            <div class="meta">
              <div class="meta-label">协议端</div>
              <div class="meta-value">{{ bot?.version || '—' }}</div>
            </div>
            <div class="meta">
              <div class="meta-label">在线时长</div>
              <div class="meta-value">{{ uptimeText }}</div>
            </div>
            <div class="meta">
              <div class="meta-label">收 / 发</div>
              <div class="meta-value">{{ fmt(bot?.msg_received) }} / {{ fmt(bot?.msg_sent) }}</div>
            </div>
            <div class="meta">
              <div class="meta-label">控制台</div>
              <div class="meta-value">
                <n-tag :type="statusType" size="small">{{ statusLabel }}</n-tag>
                <n-tag :type="authorityType" size="small" style="margin-left: 6px">{{ authorityLabel }}</n-tag>
              </div>
            </div>
          </div>
        </div>
      </n-spin>
    </n-card>

    <!-- 统计网格 -->
    <n-grid cols="2 s:3 m:4 l:7" responsive="screen" :x-gap="16" :y-gap="16">
      <n-grid-item v-for="s in statCards" :key="s.label">
        <n-card content-style="padding: 16px">
          <div class="stat-label">{{ s.label }}</div>
          <div class="stat-value">{{ s.value }}</div>
        </n-card>
      </n-grid-item>
    </n-grid>

    <!-- 近 30 天消息量 -->
    <n-card style="margin-top: 16px" content-style="padding: 16px 20px 12px">
      <div style="display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 12px">
        <span style="font-size: 14px; font-weight: 600">近 30 天消息量</span>
        <span style="font-size: 12px; color: #888">峰值 {{ fmt(chart.max) }}／天</span>
      </div>
      <svg
        ref="svgEl"
        :viewBox="`0 0 ${chart.width} ${chart.height}`"
        style="width: 100%; height: 200px; display: block"
        role="img"
        aria-label="近 30 天每日消息量柱状图"
      >
        <!-- 顶部峰值参考线 -->
        <line
          :x1="0"
          :y1="chart.top"
          :x2="chart.width"
          :y2="chart.top"
          stroke="#ffffff14"
          stroke-width="1"
        />
        <!-- 柱体 -->
        <g v-for="b in chart.bars" :key="b.date">
          <rect
            :x="b.x"
            :y="b.y"
            :width="b.w"
            :height="b.h"
            rx="2"
            fill="#63e2b7"
            :opacity="b.count > 0 ? 0.9 : 0.12"
          >
            <title>{{ b.date }}：{{ fmt(b.count) }} 条</title>
          </rect>
        </g>
        <!-- 基线 -->
        <line
          :x1="0"
          :y1="chart.baseY"
          :x2="chart.width"
          :y2="chart.baseY"
          stroke="#ffffff22"
          stroke-width="1"
        />
        <!-- 稀疏日期标签 -->
        <text
          v-for="t in chart.ticks"
          :key="t.date"
          :x="t.x"
          :y="chart.height - 4"
          fill="#888"
          font-size="11"
          text-anchor="middle"
        >{{ t.label }}</text>
      </svg>
    </n-card>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from 'vue'
import {
  NAlert,
  NAvatar,
  NButton,
  NCard,
  NGrid,
  NGridItem,
  NSpin,
  NTag,
} from 'naive-ui'
import { send, store, wsStatus } from '../ws'
import { getAuthority } from '../auth'

interface BotInfo {
  uin: number
  nickname: string | null
  avatar: string
  online: boolean
  version: string | null
  uptime_secs: number
  msg_received: number | null
  msg_sent: number | null
}

interface StatsInfo {
  users: number
  total_messages: number
  today_messages: number
  groups: number
  friends: number
}

interface DailyMessage {
  date: string
  count: number
}

interface Overview {
  bot: BotInfo
  stats: StatsInfo
  daily_messages: DailyMessage[]
}

const bot = ref<BotInfo | null>(null)
const stats = ref<StatsInfo | null>(null)
const daily = ref<DailyMessage[]>([])
const loading = ref(false)
const loadError = ref('')

async function load() {
  loading.value = true
  loadError.value = ''
  try {
    const res = (await send('overview', {})) as Overview
    bot.value = res.bot
    stats.value = res.stats
    daily.value = res.daily_messages ?? []
  } catch (e) {
    loadError.value = `加载总览失败：${String(e)}`
  } finally {
    loading.value = false
  }
}

// 柱状图按 SVG 实际像素宽度作坐标(viewBox=测得宽度×高度，1:1 不拉伸)。监听容器宽变化重算。
const svgEl = ref<SVGSVGElement | null>(null)
const svgWidth = ref(900)
let ro: ResizeObserver | null = null

onMounted(() => {
  load()
  if (svgEl.value) {
    ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width
      if (w && w > 0) svgWidth.value = w
    })
    ro.observe(svgEl.value)
    const w = svgEl.value.clientWidth
    if (w > 0) svgWidth.value = w
  }
})

onBeforeUnmount(() => {
  ro?.disconnect()
  ro = null
})

// 近 30 天轴(今天往前 30 天)，把 daily_messages 映射到各天(缺失补 0)，按峰值缩放成柱体。
const chart = computed(() => {
  const height = 200
  const width = svgWidth.value
  const padTop = 16 // 顶部留白(峰值线)
  const padBottom = 20 // 底部留给日期标签
  const baseY = height - padBottom
  const top = padTop
  const plotH = baseY - top

  // 后端返回的有记录天 → date→count 表
  const byDate = new Map<string, number>()
  for (const d of daily.value) byDate.set(d.date, d.count)

  // 今天往前 30 天的日期串(本地时区)
  const days: string[] = []
  const now = new Date()
  for (let i = 29; i >= 0; i--) {
    const d = new Date(now)
    d.setDate(now.getDate() - i)
    const p = (n: number) => String(n).padStart(2, '0')
    days.push(`${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`)
  }

  const counts = days.map((d) => byDate.get(d) ?? 0)
  const max = Math.max(1, ...counts)

  const n = days.length
  const slot = width / n
  const barW = Math.max(1, slot * 0.7)
  const gap = (slot - barW) / 2

  const bars = days.map((date, i) => {
    const count = counts[i]
    const h = (count / max) * plotH
    return {
      date,
      count,
      x: i * slot + gap,
      y: baseY - h,
      w: barW,
      h,
    }
  })

  // 稀疏日期标签:每 6 天一个(含末尾),只取 MM-DD。
  const ticks = days
    .map((date, i) => ({ date, i }))
    .filter(({ i }) => i % 6 === 0 || i === n - 1)
    .map(({ date, i }) => ({
      date,
      x: i * slot + slot / 2,
      label: date.slice(5),
    }))

  return { width, height, baseY, top, bars, ticks, max }
})

// 千分位，null/undefined → —
function fmt(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—'
  return n.toLocaleString('zh-CN')
}

// 秒数 → 「3天 4小时 12分」
const uptimeText = computed(() => {
  const secs = bot.value?.uptime_secs
  if (secs === undefined || secs === null) return '—'
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const parts: string[] = []
  if (d > 0) parts.push(`${d}天`)
  if (h > 0 || d > 0) parts.push(`${h}小时`)
  parts.push(`${m}分`)
  return parts.join(' ')
})

// 控制台 WebSocket 连接状态
const statusLabel = computed(() => {
  switch (wsStatus.value) {
    case 'connected': return '已连接'
    case 'connecting': return '连接中'
    case 'error': return '出错'
    default: return '未连接'
  }
})
const statusType = computed((): 'success' | 'warning' | 'error' | 'default' => {
  switch (wsStatus.value) {
    case 'connected': return 'success'
    case 'connecting': return 'warning'
    case 'error': return 'error'
    default: return 'default'
  }
})

// 当前账户权限
const authority = computed(() => getAuthority())
const authorityLabel = computed(() => {
  switch (authority.value) {
    case 5: return '主人'
    case 4: return '超级用户'
    case 1: return '已登录'
    default: return '未登录'
  }
})
const authorityType = computed((): 'success' | 'warning' | 'default' => {
  if (authority.value >= 4) return 'success'
  if (authority.value >= 1) return 'warning'
  return 'default'
})

// 插件数 / 待审数取自既有 DataService store
const pluginCount = computed(() => {
  const p = store['plugins'] as { plugins?: unknown[] } | undefined
  return p?.plugins?.length ?? 0
})
const reviewCount = computed(() => {
  const r = store['review'] as { items?: unknown[] } | undefined
  return r?.items?.length ?? 0
})

const statCards = computed(() => [
  { label: '用户数', value: fmt(stats.value?.users) },
  { label: '今日消息', value: fmt(stats.value?.today_messages) },
  { label: '总消息', value: fmt(stats.value?.total_messages) },
  { label: '群', value: fmt(stats.value?.groups) },
  { label: '好友', value: fmt(stats.value?.friends) },
  { label: '插件', value: fmt(pluginCount.value) },
  { label: '待审', value: fmt(reviewCount.value) },
])
</script>

<style scoped>
.meta-label {
  font-size: 12px;
  color: #888;
  margin-bottom: 4px;
}
.meta-value {
  font-size: 15px;
  font-weight: 500;
}
.stat-label {
  font-size: 13px;
  color: #888;
  margin-bottom: 8px;
}
.stat-value {
  font-size: 28px;
  font-weight: 600;
  line-height: 1.1;
}
</style>
