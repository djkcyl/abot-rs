import { reactive, ref } from 'vue'

// 可读写的 store: store[key] = value
export const store = reactive<Record<string, unknown>>({})

// patch 追加型 store（如日志）的封顶行数，超出丢弃最早的。
const PATCH_CAP = 1000

// 连接状态: 'disconnected' | 'connecting' | 'connected' | 'error'
export const wsStatus = ref<'disconnected' | 'connecting' | 'connected' | 'error'>('disconnected')

type PendingRpc = {
  resolve: (value: unknown) => void
  reject: (reason: unknown) => void
  timer: ReturnType<typeof setTimeout>
}

const pending = new Map<string, PendingRpc>()
let socket: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let pingTimer: ReturnType<typeof setInterval> | null = null
let reconnectDelay = 1000
let currentToken = ''

function clearTimers() {
  if (pingTimer !== null) {
    clearInterval(pingTimer)
    pingTimer = null
  }
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
}

function scheduleReconnect() {
  clearTimers()
  wsStatus.value = 'disconnected'
  reconnectTimer = setTimeout(() => {
    reconnectDelay = Math.min(reconnectDelay * 2, 30000)
    connect(currentToken)
  }, reconnectDelay)
}

export function connect(token: string) {
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return
  }
  currentToken = token
  clearTimers()

  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const url = `${proto}://${location.host}/api/ws?token=${encodeURIComponent(token)}`

  wsStatus.value = 'connecting'
  socket = new WebSocket(url)

  socket.onopen = () => {
    reconnectDelay = 1000
    wsStatus.value = 'connected'
    // 定时 ping
    pingTimer = setInterval(() => {
      if (socket && socket.readyState === WebSocket.OPEN) {
        socket.send(JSON.stringify({ type: 'ping' }))
      }
    }, 30000)
  }

  socket.onmessage = (event: MessageEvent) => {
    let msg: Record<string, unknown>
    try {
      msg = JSON.parse(event.data as string) as Record<string, unknown>
    } catch {
      return
    }

    const type = msg.type as string
    if (type === 'data') {
      const key = msg.key as string
      store[key] = msg.value
    } else if (type === 'patch') {
      // 增量:若 store[key] 与新值都是数组则追加并封顶(日志走这条),否则覆盖。
      const key = msg.key as string
      const incoming = msg.value
      const existing = store[key]
      if (Array.isArray(incoming) && (existing === undefined || Array.isArray(existing))) {
        const merged = Array.isArray(existing) ? existing.concat(incoming) : incoming.slice()
        // 封顶 1000 行，丢弃最早的。
        store[key] = merged.length > PATCH_CAP ? merged.slice(merged.length - PATCH_CAP) : merged
      } else {
        store[key] = incoming
      }
    } else if (type === 'response') {
      const id = msg.id as string
      const entry = pending.get(id)
      if (entry) {
        clearTimeout(entry.timer)
        pending.delete(id)
        if (Object.prototype.hasOwnProperty.call(msg, 'error')) {
          entry.reject(msg.error)
        } else {
          entry.resolve(msg.value)
        }
      }
    }
    // type === 'pong' → 忽略
  }

  socket.onclose = () => {
    clearTimers()
    wsStatus.value = 'disconnected'
    scheduleReconnect()
  }

  socket.onerror = () => {
    wsStatus.value = 'error'
    socket?.close()
  }
}

let rpcSeq = 0

export function send(event: string, args: unknown): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const dispatch = () => {
      const id = `rpc-${++rpcSeq}-${Date.now()}`
      const timer = setTimeout(() => {
        pending.delete(id)
        reject(new Error(`RPC 超时: ${event}`))
      }, 60000)
      pending.set(id, { resolve, reject, timer })
      socket!.send(JSON.stringify({ type: 'send', id, event, args }))
    }
    // 连接没就绪时等它就位再发（刚登录/重连时页面 mount 即调用会撞上这一刻），最多等 5 秒。
    const ready = () => socket && socket.readyState === WebSocket.OPEN
    if (ready()) {
      dispatch()
      return
    }
    let waited = 0
    const iv = setInterval(() => {
      if (ready()) {
        clearInterval(iv)
        dispatch()
      } else if ((waited += 100) >= 5000) {
        clearInterval(iv)
        reject(new Error('WebSocket 未连接'))
      }
    }, 100)
  })
}

export function disconnect() {
  clearTimers()
  if (socket) {
    socket.onclose = null
    socket.close()
    socket = null
  }
  wsStatus.value = 'disconnected'
}
