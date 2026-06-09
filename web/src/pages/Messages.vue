<template>
  <div style="padding: 24px; display: flex; gap: 16px; height: calc(100vh - 48px); box-sizing: border-box">
    <!-- 左侧会话列表 -->
    <n-card style="width: 280px; flex-shrink: 0; display: flex; flex-direction: column" content-style="padding: 0; display: flex; flex-direction: column; flex: 1; overflow: hidden">
      <template #header>
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span>会话</span>
          <n-button text size="tiny" :loading="loadingConvs" @click="loadConversations">刷新</n-button>
        </div>
      </template>
      <div style="padding: 8px 12px">
        <n-input v-model:value="filter" size="small" clearable placeholder="搜索会话">
          <template #prefix>
            <span style="color: #666">🔍</span>
          </template>
        </n-input>
      </div>
      <div style="flex: 1; overflow: auto">
        <n-spin v-if="loadingConvs && conversations.length === 0" size="small" style="display: block; text-align: center; padding: 16px" />
        <n-empty v-else-if="conversations.length === 0" description="没有会话" style="padding: 24px" />
        <template v-else>
          <!-- 群 -->
          <template v-if="filteredGroups.length">
            <div style="padding: 6px 12px; font-size: 12px; color: #888; background: rgba(255,255,255,0.02)">群（{{ filteredGroups.length }}）</div>
            <n-list clickable>
              <n-list-item
                v-for="c in filteredGroups"
                :key="convKey(c)"
                :style="isSelected(c) ? { background: 'rgba(99,226,183,0.1)' } : {}"
                @click="selectConversation(c)"
              >
                <div>
                  <div style="font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ c.name }}</div>
                  <div style="font-size: 12px; color: #999; margin-top: 2px">{{ c.id }} · {{ c.count }} 条</div>
                </div>
              </n-list-item>
            </n-list>
          </template>
          <!-- 私聊 -->
          <template v-if="filteredPrivates.length">
            <div style="padding: 6px 12px; font-size: 12px; color: #888; background: rgba(255,255,255,0.02)">私聊（{{ filteredPrivates.length }}）</div>
            <n-list clickable>
              <n-list-item
                v-for="c in filteredPrivates"
                :key="convKey(c)"
                :style="isSelected(c) ? { background: 'rgba(99,226,183,0.1)' } : {}"
                @click="selectConversation(c)"
              >
                <div>
                  <div style="font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ c.name }}</div>
                  <div style="font-size: 12px; color: #999; margin-top: 2px">{{ c.id }} · {{ c.count }} 条</div>
                </div>
              </n-list-item>
            </n-list>
          </template>
          <n-empty v-if="!filteredGroups.length && !filteredPrivates.length" description="无匹配会话" style="padding: 24px" />
        </template>
      </div>
    </n-card>

    <!-- 右侧聊天面板 -->
    <n-card
      style="flex: 1; overflow: hidden; display: flex; flex-direction: column"
      content-style="padding: 0; display: flex; flex-direction: column; flex: 1; overflow: hidden"
    >
      <template #header>
        <div style="display: flex; align-items: center; justify-content: space-between">
          <span v-if="selected">
            {{ selected.name }}
            <span style="font-size: 12px; color: #999">（{{ selected.kind === 'group' ? '群' : '私聊' }} {{ selected.id }}）</span>
          </span>
          <span v-else style="color: #999">选择一个会话</span>
          <n-button v-if="selected" size="small" :loading="loadingMsgs" @click="loadMessages">刷新</n-button>
        </div>
      </template>

      <template v-if="!selected">
        <div style="padding: 40px; text-align: center; color: #666; flex: 1">从左侧选择一个会话查看聊天记录</div>
      </template>

      <template v-else>
        <!-- 消息历史 -->
        <div ref="historyEl" style="flex: 1; overflow: auto; padding: 12px">
          <n-spin v-if="loadingMsgs && messages.length === 0" size="small" style="display: block; text-align: center; padding: 16px" />
          <n-empty v-else-if="messages.length === 0" description="暂无消息" style="padding: 24px" />
          <div
            v-for="m in messages"
            :key="m.id"
            :style="{ display: 'flex', flexDirection: 'column', alignItems: m.from_self ? 'flex-end' : 'flex-start', marginBottom: '12px' }"
          >
            <div style="font-size: 12px; color: #777; margin-bottom: 2px; padding: 0 4px">
              <span>{{ m.from_self ? '机器人' : (m.nickname || m.uin) }}</span>
              <span style="margin-left: 6px; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace">{{ fmtTime(m.time) }}</span>
            </div>
            <div
              :style="bubbleStyle(m.from_self)"
            >{{ renderContent(m.content) }}</div>
          </div>
        </div>

        <!-- 发送框 -->
        <div style="border-top: 1px solid rgba(255,255,255,0.08); padding: 10px 12px">
          <template v-if="canSend">
            <n-input
              v-model:value="draft"
              type="textarea"
              :autosize="{ minRows: 2, maxRows: 5 }"
              placeholder="输入消息，Ctrl + Enter 发送"
              @keydown="onDraftKeydown"
            />
            <div style="display: flex; justify-content: flex-end; margin-top: 8px">
              <n-button type="primary" size="small" :loading="sending" @click="onSend">发送</n-button>
            </div>
          </template>
          <div v-else style="text-align: center; color: #888; font-size: 13px; padding: 6px">发送消息需要主人权限</div>
        </div>
      </template>
    </n-card>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick } from 'vue'
import { NCard, NList, NListItem, NButton, NInput, NSpin, NEmpty, useMessage } from 'naive-ui'
import { send } from '../ws'
import { getAuthority } from '../auth'

type Kind = 'group' | 'private'

interface Conversation {
  kind: Kind
  id: number
  name: string
  count: number
}
interface Segment {
  type: string
  data?: Record<string, unknown>
}
interface ChatMessage {
  id: number
  uin: number
  nickname: string | null
  from_self: boolean
  content: Segment[] | null
  time: string
}

const message = useMessage()
const canSend = getAuthority() >= 5

const conversations = ref<Conversation[]>([])
const selected = ref<Conversation | null>(null)
const messages = ref<ChatMessage[]>([])
const loadingConvs = ref(false)
const loadingMsgs = ref(false)
const filter = ref('')
const draft = ref('')
const sending = ref(false)
const historyEl = ref<HTMLElement | null>(null)

function convKey(c: Conversation): string {
  return `${c.kind}:${c.id}`
}
function isSelected(c: Conversation): boolean {
  return selected.value !== null && convKey(selected.value) === convKey(c)
}

const filteredGroups = computed(() => {
  const kw = filter.value.trim().toLowerCase()
  return conversations.value.filter(
    (c) => c.kind === 'group' && (!kw || c.name.toLowerCase().includes(kw) || String(c.id).includes(kw)),
  )
})
const filteredPrivates = computed(() => {
  const kw = filter.value.trim().toLowerCase()
  return conversations.value.filter(
    (c) => c.kind === 'private' && (!kw || c.name.toLowerCase().includes(kw) || String(c.id).includes(kw)),
  )
})

async function loadConversations() {
  loadingConvs.value = true
  try {
    const res = (await send('chatlog/conversations', {})) as { conversations: Conversation[] }
    conversations.value = res.conversations ?? []
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    loadingConvs.value = false
  }
}

function selectConversation(c: Conversation) {
  if (isSelected(c)) return
  selected.value = c
  messages.value = []
  loadMessages()
}

async function loadMessages() {
  if (!selected.value) return
  loadingMsgs.value = true
  try {
    const res = (await send('chatlog/query', { kind: selected.value.kind, id: selected.value.id })) as {
      messages: ChatMessage[]
    }
    messages.value = res.messages ?? []
    // 历史按时间序,滚到底部看最新。
    await nextTick()
    if (historyEl.value) historyEl.value.scrollTop = historyEl.value.scrollHeight
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    loadingMsgs.value = false
  }
}

function onDraftKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault()
    onSend()
  }
}

async function onSend() {
  if (!selected.value || !canSend) return
  const text = draft.value.trim()
  if (!text) {
    message.warning('请输入消息内容')
    return
  }
  sending.value = true
  try {
    await send('message/send', {
      target_type: selected.value.kind,
      target_id: selected.value.id,
      text,
    })
    message.success('已发送')
    draft.value = ''
    // 重新拉历史,让刚发出的消息在被记录后显示出来。
    await loadMessages()
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    sending.value = false
  }
}

// 机器人自己的消息靠右、绿底;别人靠左、灰底。
function bubbleStyle(fromSelf: boolean): Record<string, string> {
  const base: Record<string, string> = {
    maxWidth: '70%',
    padding: '8px 12px',
    borderRadius: '8px',
    lineHeight: '1.5',
    wordBreak: 'break-word',
    whiteSpace: 'pre-wrap',
    fontSize: '14px',
  }
  if (fromSelf) {
    base.background = 'rgba(99,226,183,0.18)'
    base.color = '#d9f7ec'
    base.border = '1px solid rgba(99,226,183,0.3)'
  } else {
    base.background = 'rgba(255,255,255,0.06)'
    base.color = '#d9d9d9'
    base.border = '1px solid rgba(255,255,255,0.08)'
  }
  return base
}

// rfc3339 → HH:MM:SS（本地时区）。
function fmtTime(s: string): string {
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

// OneBot 段数组 → 可读文本。识别常见段,其余回退成「[类型]」。空内容 → （空）。
function renderContent(content: Segment[] | null): string {
  if (!Array.isArray(content) || content.length === 0) return '（空）'
  const parts: string[] = []
  for (const seg of content) {
    const type = seg?.type ?? ''
    const data = (seg?.data ?? {}) as Record<string, unknown>
    switch (type) {
      case 'text':
        parts.push(String(data.text ?? ''))
        break
      case 'at':
        parts.push(`@${data.qq ?? ''}`)
        break
      case 'image':
        parts.push('[图片]')
        break
      case 'face':
        parts.push('[表情]')
        break
      case 'reply':
        parts.push('[回复]')
        break
      case 'record':
        parts.push('[语音]')
        break
      case 'video':
        parts.push('[视频]')
        break
      case 'forward':
        parts.push('[合并转发]')
        break
      default:
        parts.push(`[${type}]`)
    }
  }
  const text = parts.join('')
  return text.length > 0 ? text : '（空）'
}

loadConversations()
</script>
