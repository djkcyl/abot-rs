<template>
  <div style="padding: 24px">
    <h2 style="margin-bottom: 24px">插件</h2>
    <template v-if="!pluginsData">
      <n-spin size="small" style="display: block; text-align: center; padding: 24px" />
    </template>
    <template v-else>
      <!-- 按分类分组,每组一张表;空分组不显示 -->
      <div v-for="group in groups" :key="group.category" style="margin-bottom: 24px">
        <h3 style="margin: 0 0 12px; font-size: 15px; color: #888">{{ group.label }}</h3>
        <n-data-table
          :columns="columns"
          :data="group.rows"
          :row-key="rowKey"
          striped
        />
      </div>
      <n-empty v-if="groups.length === 0" description="没有插件" style="padding: 24px" />
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, h } from 'vue'
import { NDataTable, NSwitch, NTag, NEmpty, NSpin, useMessage } from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { store, send } from '../ws'
import { getAuthority } from '../auth'

interface Command {
  id: string
  name: string
  words: string[]
  description: string
  enabled: boolean
  can_disable: boolean
  hidden: boolean
  key: string
}

interface Plugin {
  key: string
  name: string
  category: string
  description: string
  hidden: boolean
  enabled: boolean
  can_disable: boolean
  commands?: Command[]
}

interface PluginsData {
  plugins: Plugin[]
}

const message = useMessage()

const pluginsData = computed(() => store['plugins'] as PluginsData | undefined)

const rows = computed<Plugin[]>(() => pluginsData.value?.plugins ?? [])

// 分类展示顺序 + 中文名,与 help 插件的 CATEGORY_ORDER 一致;后端 category 是 Debug 形(首字母大写)。
const CATEGORY_ORDER: { category: string; label: string }[] = [
  { category: 'Fun', label: '娱乐' },
  { category: 'Tool', label: '工具' },
  { category: 'User', label: '用户' },
  { category: 'Admin', label: '管理' },
  { category: 'Push', label: '推送' },
  { category: 'Core', label: '核心' },
]

// 按分类分组(空组跳过);未在顺序表里的分类兜底排到末尾。
const groups = computed(() => {
  const byCat = new Map<string, Plugin[]>()
  for (const p of rows.value) {
    const list = byCat.get(p.category) ?? []
    list.push(p)
    byCat.set(p.category, list)
  }
  const out: { category: string; label: string; rows: Plugin[] }[] = []
  for (const { category, label } of CATEGORY_ORDER) {
    const list = byCat.get(category)
    if (list && list.length) {
      out.push({ category, label, rows: list })
      byCat.delete(category)
    }
  }
  // 兜底:顺序表外的分类(若有),原样以分类名作标题列在末尾。
  for (const [category, list] of byCat) {
    if (list.length) out.push({ category, label: category, rows: list })
  }
  return out
})

function rowKey(row: Plugin) {
  return row.key
}

// 翻转某插件总开关:乐观更新本地状态,失败回滚并提示。
async function toggle(row: Plugin, next: boolean) {
  const prev = row.enabled
  row.enabled = next
  try {
    await send('plugin/toggle', { key: row.key, enabled: next })
    message.success(next ? '已启用' : '已停用')
  } catch (e) {
    row.enabled = prev
    message.error(typeof e === 'string' ? e : String(e))
  }
}

// 翻转某条命令子开关:同样乐观更新 + 失败回滚。
async function toggleCommand(cmd: Command, next: boolean) {
  const prev = cmd.enabled
  cmd.enabled = next
  try {
    await send('command/toggle', { key: cmd.key, enabled: next })
    message.success(next ? '已启用' : '已停用')
  } catch (e) {
    cmd.enabled = prev
    message.error(typeof e === 'string' ? e : String(e))
  }
}

// 命令的别名:words[0] 是主词,其余为别名,用顿号连接。
function aliases(cmd: Command): string {
  const ws = cmd.words ?? []
  return ws.length > 1 ? ws.slice(1).join('、') : ''
}

// 展开区:列出该插件名下可见命令(隐藏命令不展示),每条给名字/别名/简介 + 子开关。
function renderExpand(row: Plugin) {
  const cmds = (row.commands ?? []).filter((c) => !c.hidden)
  if (cmds.length === 0) {
    return h('div', { style: 'padding: 8px 0; color: #888' }, '该插件没有可单独开关的命令。')
  }
  return h(
    NDataTable,
    {
      size: 'small',
      bordered: false,
      data: cmds,
      rowKey: (c: Command) => c.key,
      columns: commandColumns,
    },
  )
}

const commandColumns: DataTableColumns<Command> = [
  {
    title: '启用',
    key: 'enabled',
    width: 80,
    render(cmd) {
      // 不可停用的命令(如开关管理自身):用「常驻」标签表明始终启用,不给开关。
      if (!cmd.can_disable) {
        return h(NTag, { type: 'success', size: 'small', bordered: false }, { default: () => '常驻' })
      }
      return h(NSwitch, {
        value: cmd.enabled,
        size: 'small',
        disabled: getAuthority() < 4,
        'onUpdate:value': (val: boolean) => toggleCommand(cmd, val),
      })
    },
  },
  { title: '命令', key: 'name', width: 120 },
  {
    title: '别名',
    key: '__aliases__',
    width: 160,
    render(cmd) {
      const a = aliases(cmd)
      return a || h('span', { style: 'color:#666' }, '—')
    },
  },
  {
    title: '描述',
    key: 'description',
    ellipsis: { tooltip: true },
    render(cmd) {
      return cmd.description || h('span', { style: 'color:#666' }, '—')
    },
  },
]

const columns: DataTableColumns<Plugin> = [
  {
    type: 'expand',
    // 名下无可见命令的插件不显示展开箭头。
    expandable: (row) => (row.commands ?? []).some((c) => !c.hidden),
    renderExpand,
  },
  {
    title: '启用',
    key: 'enabled',
    width: 80,
    render(row) {
      // 核心/常驻插件不可停用:用「常驻」标签表明始终启用,不给开关(避免禁用开关的歧义)。
      if (!row.can_disable) {
        return h(NTag, { type: 'success', size: 'small', bordered: false }, { default: () => '常驻' })
      }
      // 普通插件:无写权限者只读查看(禁用交互),有权限者可切换。
      return h(NSwitch, {
        value: row.enabled,
        disabled: getAuthority() < 4,
        'onUpdate:value': (val: boolean) => toggle(row, val),
      })
    },
  },
  {
    title: '名称',
    key: 'name',
    width: 120,
    sorter: 'default',
  },
  {
    title: 'Key',
    key: 'key',
    width: 160,
    sorter: 'default',
  },
  {
    title: '分类',
    key: 'category',
    width: 100,
    sorter: 'default',
  },
  {
    title: '描述',
    key: 'description',
    ellipsis: { tooltip: true },
  },
  {
    title: '隐藏',
    key: 'hidden',
    width: 70,
    render(row) {
      return row.hidden ? '是' : '否'
    },
  },
]
</script>
