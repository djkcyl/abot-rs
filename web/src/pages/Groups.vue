<template>
  <div style="padding: 24px">
    <h2 style="margin-bottom: 24px">群管理</h2>

    <n-alert v-if="loadError" type="error" style="margin-bottom: 16px" :title="loadError" closable @close="loadError = ''" />

    <div style="display: flex; gap: 16px; height: calc(100vh - 140px)">
      <!-- 左：群列表 -->
      <n-card style="width: 300px; flex-shrink: 0; overflow: auto" content-style="padding: 0">
        <template #header>
          <div style="display: flex; align-items: center; justify-content: space-between">
            <span>群（{{ groups.length }}）</span>
            <n-button size="tiny" :loading="loading" @click="refreshAll">刷新</n-button>
          </div>
        </template>
        <n-spin v-if="loading && groups.length === 0" size="small" style="display: block; text-align: center; padding: 16px" />
        <n-empty v-else-if="groups.length === 0" description="没有群" style="padding: 24px" />
        <n-list v-else clickable>
          <n-list-item
            v-for="g in groups"
            :key="g.group_id"
            :style="selectedGroup?.group_id === g.group_id ? { background: 'rgba(99,226,183,0.1)' } : {}"
            @click="selectGroup(g)"
          >
            <div>
              <div style="display: flex; align-items: center; gap: 8px">
                <span style="font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ g.name }}</span>
                <n-tag v-if="botRoleTag(g.bot_role)" :type="botRoleTag(g.bot_role)!.type" size="tiny" :bordered="false">
                  {{ botRoleTag(g.bot_role)!.label }}
                </n-tag>
              </div>
              <div style="font-size: 12px; color: #999; margin-top: 2px">{{ g.group_id }} · {{ g.member_count }} 人</div>
            </div>
          </n-list-item>
        </n-list>
      </n-card>

      <!-- 右：群成员 -->
      <n-card style="flex: 1; overflow: hidden; display: flex; flex-direction: column" content-style="padding: 12px; display: flex; flex-direction: column; flex: 1; overflow: hidden">
        <template v-if="!selectedGroup">
          <div style="padding: 40px; text-align: center; color: #666">从左侧选择一个群</div>
        </template>
        <template v-else>
          <!-- 群级控制 -->
          <div style="display: flex; align-items: center; gap: 16px; margin-bottom: 12px; flex-wrap: wrap">
            <span style="font-weight: 500">{{ selectedGroup.name }}</span>
            <n-tag v-if="botRoleTag(botRole)" :type="botRoleTag(botRole)!.type" size="small" :bordered="false">
              本机{{ botRoleTag(botRole)!.label }}
            </n-tag>
            <!-- 全体禁言:仅群主／管理员可操作 -->
            <template v-if="botRole === 'owner' || botRole === 'admin'">
              <span v-if="wholeMute !== null" style="display: flex; align-items: center; gap: 6px">
                全体禁言
                <n-switch :value="wholeMute" :loading="wholeMuteLoading" @update:value="onWholeMute" />
              </span>
              <template v-else>
                <n-popconfirm positive-text="确定" negative-text="取消" @positive-click="onWholeMute(true)">
                  <template #trigger>
                    <n-button size="small" :loading="wholeMuteLoading">全体禁言</n-button>
                  </template>
                  确定开启全体禁言？
                </n-popconfirm>
                <n-button size="small" :loading="wholeMuteLoading" @click="onWholeMute(false)">解除全体禁言</n-button>
              </template>
            </template>
            <n-button size="small" :loading="membersLoading" @click="loadMembers(selectedGroup!.group_id)">刷新成员</n-button>
            <n-popconfirm v-if="authority >= 5" positive-text="确定" negative-text="取消" @positive-click="onLeave(false)">
              <template #trigger>
                <n-button size="small" type="error">退群</n-button>
              </template>
              确定退出该群？
            </n-popconfirm>
          </div>

          <div style="flex: 1; overflow: auto">
            <n-data-table
              :columns="memberColumns"
              :data="members"
              :loading="membersLoading"
              size="small"
              :max-height="'calc(100vh - 280px)'"
            />
          </div>
        </template>
      </n-card>
    </div>

    <!-- 成员操作弹窗（禁言／改名片／设头衔共用一个，按 editKind 切换） -->
    <n-modal v-model:show="showEdit" preset="card" :title="editTitle" style="width: 420px; max-width: 92vw">
      <n-input-number
        v-if="editKind === 'mute'"
        v-model:value="editNum"
        :min="0"
        style="width: 100%"
        placeholder="禁言时长（秒），0 为解禁"
      />
      <n-input
        v-else
        v-model:value="editText"
        :placeholder="editKind === 'card' ? '名片' : '头衔'"
        @keyup.enter="confirmEdit"
      />
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="showEdit = false">取消</n-button>
          <n-button type="primary" :loading="editSaving" @click="confirmEdit">确定</n-button>
        </div>
      </template>
    </n-modal>

    <!-- 踢出确认 -->
    <n-modal
      :show="kickTarget !== null"
      preset="card"
      title="踢出成员"
      style="width: 380px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) kickTarget = null }"
    >
      <span v-if="kickTarget">确定踢出 {{ kickTarget.card || kickTarget.nickname }}？</span>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="kickTarget = null">取消</n-button>
          <n-button type="error" :loading="kicking" @click="confirmKick">确定</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, h, onMounted } from 'vue'
import {
  NCard,
  NList,
  NListItem,
  NDataTable,
  NButton,
  NSwitch,
  NSpin,
  NEmpty,
  NAlert,
  NTag,
  NPopconfirm,
  NInput,
  NInputNumber,
  NDropdown,
  NModal,
  useMessage,
} from 'naive-ui'
import type { DataTableColumns, DropdownOption } from 'naive-ui'
import { send } from '../ws'
import { getAuthority } from '../auth'

type Role = 'owner' | 'admin' | 'member'

interface Group {
  group_id: number
  name: string
  member_count: number
  owner_id: number | null
  bot_role: Role | null
}

interface Member {
  uin: number
  nickname: string
  card: string
  role: Role
  level: number
  title: string
  join_time: number
  last_sent_time: number
  mute_end_time: number | null
}

const message = useMessage()
const authority = getAuthority()

const groups = ref<Group[]>([])
const loading = ref(false)
const loadError = ref('')

const selectedGroup = ref<Group | null>(null)
const members = ref<Member[]>([])
const membersLoading = ref(false)
// 机器人在当前群的角色与 QQ 号,来自 group/members(更权威);列表里的 bot_role 是兜底。
const botRole = ref<Role | null>(null)
const botUin = ref<number | null>(null)
// 全体禁言:协议端能回状态(get_group_info 的 shut_up_all_time)就给开关反映真实状态,
// 回不了(null)就退回「开启/解除」两个按钮——不假装知道未知的状态。
const wholeMuteLoading = ref(false)
// null = 协议端未回全禁状态(未知);true/false = 已知当前是否全禁。
const wholeMute = ref<boolean | null>(null)

const roleLabel: Record<string, string> = {
  owner: '群主',
  admin: '管理员',
  member: '成员',
}

// 角色 → tag 颜色;成员=default,无角色(null)不显示 tag。
function botRoleTag(role: Role | null): { type: 'success' | 'info' | 'default'; label: string } | null {
  switch (role) {
    case 'owner':
      return { type: 'success', label: '群主' }
    case 'admin':
      return { type: 'info', label: '管理员' }
    case 'member':
      return { type: 'default', label: '成员' }
    default:
      return null
  }
}

// unix 秒 → YYYY-MM-DD（本地时区）。0／无效 → —。
function fmtDate(secs: number): string {
  if (!secs || secs <= 0) return '—'
  const d = new Date(secs * 1000)
  if (Number.isNaN(d.getTime())) return '—'
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

const memberColumns = computed<DataTableColumns<Member>>(() => [
  {
    title: '昵称／名片',
    key: 'name',
    ellipsis: { tooltip: true },
    render: (row) => row.card || row.nickname || '—',
  },
  {
    title: 'QQ号',
    key: 'uin',
    width: 110,
    sorter: 'default',
  },
  {
    title: '角色',
    key: 'role',
    width: 90,
    render: (row) => roleLabel[row.role] ?? row.role,
  },
  { title: '等级', key: 'level', width: 80, sorter: 'default' },
  {
    title: '头衔',
    key: 'title',
    ellipsis: { tooltip: true },
    render: (row) => row.title || '—',
  },
  {
    title: '加群时间',
    key: 'join_time',
    width: 120,
    sorter: 'default',
    render: (row) => fmtDate(row.join_time),
  },
  {
    title: '操作',
    key: '__actions__',
    width: 90,
    render: (row) => renderActions(row),
  },
])

// 角色门禁:botRole 不是群主/管理员则全无操作菜单;群主可管所有人,
// 管理员只可管普通成员;任何情况都不对自己动手。
function memberMenuOptions(row: Member): DropdownOption[] {
  const role = botRole.value
  if (role !== 'owner' && role !== 'admin') return []

  const isSelf = botUin.value !== null && row.uin === botUin.value
  if (isSelf) return []

  const target = row.role
  const canManage = role === 'owner' || (role === 'admin' && target === 'member')
  const isOwner = role === 'owner'

  const opts: DropdownOption[] = []
  if (canManage) {
    opts.push({ label: '禁言…', key: 'mute' })
    opts.push({ label: '解禁', key: 'unmute' })
    opts.push({ label: '改名片…', key: 'card' })
  }
  // 设头衔／设管理员仅群主可用。
  if (isOwner) {
    opts.push({ label: '设头衔…', key: 'title' })
    if (target !== 'owner') {
      opts.push(target === 'admin' ? { label: '撤销管理员', key: 'demote' } : { label: '设为管理员', key: 'promote' })
    }
  }
  if (canManage) {
    opts.push({ type: 'divider', key: 'd-kick' })
    opts.push({ label: '踢出', key: 'kick', props: { style: 'color:#f5222d' } })
  }
  return opts
}

function onMemberMenu(key: string, row: Member) {
  switch (key) {
    case 'mute':
      openEdit('mute', row)
      break
    case 'unmute':
      doAction('mute', row, { duration: 0 }, '已解禁')
      break
    case 'card':
      openEdit('card', row)
      break
    case 'title':
      openEdit('title', row)
      break
    case 'promote':
      doAction('admin', row, { enable: true }, '已设管理员')
      break
    case 'demote':
      doAction('admin', row, { enable: false }, '已撤管理员')
      break
    case 'kick':
      kickTarget.value = row
      break
  }
}

function renderActions(row: Member) {
  const opts = memberMenuOptions(row)
  // 菜单为空(机器人是普通成员、对象是自己、或无可用项)→ 渲染破折号。
  if (opts.length === 0) return h('span', { style: 'color:#666' }, '—')
  return h(
    NDropdown,
    {
      trigger: 'click',
      options: opts,
      onSelect: (key: string) => onMemberMenu(key, row),
    },
    {
      default: () => h(NButton, { size: 'tiny' }, { default: () => '操作 ▾' }),
    },
  )
}

async function refreshAll() {
  loading.value = true
  loadError.value = ''
  try {
    const result = (await send('contacts/list', {})) as { groups: Group[] }
    groups.value = result.groups ?? []
    // 选中的群若仍在列表里,刷新它的 bot_role 兜底值。
    if (selectedGroup.value) {
      const cur = groups.value.find((g) => g.group_id === selectedGroup.value!.group_id)
      selectedGroup.value = cur ?? null
      if (!cur) {
        members.value = []
      }
    }
  } catch (e) {
    loadError.value = typeof e === 'string' ? e : String(e)
  } finally {
    loading.value = false
  }
}

function selectGroup(g: Group) {
  selectedGroup.value = g
  botRole.value = g.bot_role
  loadMembers(g.group_id)
}

async function loadMembers(group: number) {
  membersLoading.value = true
  try {
    const resp = (await send('group/members', { group })) as {
      members: Member[]
      whole_muted: boolean | null
      bot_role: Role | null
      bot_uin: number
    }
    members.value = resp.members ?? []
    wholeMute.value = resp.whole_muted ?? null
    botRole.value = resp.bot_role ?? selectedGroup.value?.bot_role ?? null
    botUin.value = resp.bot_uin ?? null
  } catch (e) {
    members.value = []
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    membersLoading.value = false
  }
}

async function doAction(
  action: string,
  row: Member,
  extra: Record<string, unknown>,
  okMsg: string,
) {
  if (!selectedGroup.value) return
  try {
    await send('group/action', { action, group: selectedGroup.value.group_id, user: row.uin, ...extra })
    message.success(okMsg)
    await loadMembers(selectedGroup.value.group_id)
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  }
}

// ─── 成员操作弹窗（禁言／改名片／设头衔共用）───
type EditKind = 'mute' | 'card' | 'title'
const showEdit = ref(false)
const editKind = ref<EditKind>('mute')
const editRow = ref<Member | null>(null)
const editText = ref('')
const editNum = ref<number | null>(null)
const editSaving = ref(false)
const editTitle = computed(() => {
  switch (editKind.value) {
    case 'mute':
      return '禁言'
    case 'card':
      return '改名片'
    case 'title':
      return '设头衔'
  }
  return ''
})

function openEdit(kind: EditKind, row: Member) {
  editKind.value = kind
  editRow.value = row
  if (kind === 'mute') {
    editNum.value = 600
  } else if (kind === 'card') {
    editText.value = row.card ?? ''
  } else {
    editText.value = row.title ?? ''
  }
  showEdit.value = true
}

async function confirmEdit() {
  const row = editRow.value
  if (!row) return
  editSaving.value = true
  try {
    if (editKind.value === 'mute') {
      const sec = editNum.value ?? 0
      if (sec < 0) {
        message.warning('请输入禁言时长（秒）')
        editSaving.value = false
        return
      }
      await doAction('mute', row, { duration: Math.floor(sec) }, sec > 0 ? '已禁言' : '已解禁')
    } else if (editKind.value === 'card') {
      await doAction('card', row, { card: editText.value }, '名片已改')
    } else {
      await doAction('title', row, { title: editText.value }, '头衔已设')
    }
    showEdit.value = false
  } finally {
    editSaving.value = false
  }
}

// ─── 踢出确认 ───
const kickTarget = ref<Member | null>(null)
const kicking = ref(false)
async function confirmKick() {
  const row = kickTarget.value
  if (!row) return
  kicking.value = true
  try {
    await doAction('kick', row, {}, '已踢出')
    kickTarget.value = null
  } finally {
    kicking.value = false
  }
}

async function onWholeMute(enable: boolean) {
  if (!selectedGroup.value) return
  wholeMuteLoading.value = true
  try {
    await send('group/action', { action: 'whole_mute', group: selectedGroup.value.group_id, enable })
    // 已知状态时同步开关(未知模式是按钮,不必动)。
    if (wholeMute.value !== null) wholeMute.value = enable
    message.success(enable ? '已开启全体禁言' : '已解除全体禁言')
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    wholeMuteLoading.value = false
  }
}

async function onLeave(dismiss: boolean) {
  if (!selectedGroup.value) return
  const group = selectedGroup.value.group_id
  try {
    await send('group/action', { action: 'leave', group, dismiss })
    message.success('已退群')
    selectedGroup.value = null
    members.value = []
    await refreshAll()
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  }
}

onMounted(refreshAll)
</script>
