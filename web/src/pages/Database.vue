<template>
  <div style="padding: 24px; display: flex; gap: 16px; height: calc(100vh - 48px); box-sizing: border-box">
    <!-- 左侧表列表 -->
    <n-card style="width: 240px; flex-shrink: 0; overflow: auto" content-style="padding: 0">
      <template #header>数据库表</template>
      <n-spin v-if="!dbData" size="small" style="display: block; text-align: center; padding: 16px" />
      <n-empty v-else-if="dbData.tables.length === 0" description="没有表" style="padding: 24px" />
      <n-list v-else clickable>
        <n-list-item
          v-for="tbl in dbData.tables"
          :key="tbl.name"
          :style="selectedTable === tbl.name ? { background: 'rgba(99,226,183,0.1)' } : {}"
          @click="selectTable(tbl.name)"
        >
          <div>
            <div style="font-weight: 500">{{ tbl.name }}</div>
            <div style="font-size: 12px; color: #999">{{ tbl.rows }} 行</div>
          </div>
        </n-list-item>
      </n-list>
    </n-card>

    <!-- 右侧表数据 -->
    <n-card
      style="flex: 1; overflow: hidden; display: flex; flex-direction: column"
      content-style="padding: 12px; display: flex; flex-direction: column; flex: 1; overflow: hidden"
    >
      <template #header>
        <span v-if="selectedTable">{{ selectedTable }}</span>
        <span v-else style="color: #999">选择一张表</span>
      </template>

      <template v-if="!selectedTable">
        <div style="padding: 40px; text-align: center; color: #666">从左侧选择一张表查看数据</div>
      </template>

      <template v-else>
        <!-- 工具栏 -->
        <div style="display: flex; align-items: center; gap: 8px; margin-bottom: 8px; flex-wrap: wrap">
          <n-input
            v-model:value="searchInput"
            placeholder="搜索（全列）"
            size="small"
            clearable
            style="width: 200px"
            @keyup.enter="applySearch"
            @clear="applySearch"
          />
          <n-button size="small" @click="applySearch">搜索</n-button>
          <n-button size="small" @click="addFilterRow">添加筛选</n-button>
          <n-button v-if="filters.length" size="small" @click="clearFilters">清空筛选</n-button>
          <n-button size="small" :loading="querying" @click="doQuery">刷新</n-button>
          <span style="flex: 1" />
          <n-button v-if="canWrite" size="small" type="primary" @click="openInsert">插入</n-button>
        </div>

        <!-- 筛选构建器 -->
        <div v-if="filters.length" style="margin-bottom: 8px; display: flex; flex-direction: column; gap: 6px">
          <div
            v-for="(f, i) in filters"
            :key="i"
            style="display: flex; align-items: center; gap: 6px; flex-wrap: wrap"
          >
            <n-select
              v-model:value="f.column"
              size="small"
              style="width: 160px"
              :options="columnOptions"
              placeholder="列"
            />
            <n-select v-model:value="f.op" size="small" style="width: 120px" :options="opOptions" />
            <n-input
              v-if="opNeedsValue(f.op)"
              v-model:value="f.value"
              size="small"
              style="width: 180px"
              placeholder="值"
            />
            <n-button size="tiny" type="error" @click="removeFilterRow(i)">删除</n-button>
          </div>
          <div>
            <n-button size="small" type="primary" @click="applyFilters">应用筛选</n-button>
          </div>
        </div>

        <!-- 无主键提示 -->
        <n-alert
          v-if="loaded && pk.length === 0"
          type="info"
          :show-icon="false"
          style="margin-bottom: 8px; padding: 4px 12px"
        >
          该表没有主键，行不可编辑／删除（只读）。
        </n-alert>

        <!-- 数据网格 -->
        <div style="flex: 1; overflow: auto">
          <n-data-table
            :columns="gridColumns"
            :data="rows"
            :loading="querying"
            size="small"
            :max-height="'calc(100vh - 280px)'"
            :row-key="(r: Row) => rowKey(r)"
          />
        </div>

        <!-- 分页 -->
        <div style="display: flex; align-items: center; gap: 12px; margin-top: 10px; flex-wrap: wrap">
          <n-select v-model:value="limit" size="small" style="width: 110px" :options="pageSizeOptions" @update:value="onPageSize" />
          <n-button size="small" :disabled="offset === 0 || querying" @click="prevPage">上一页</n-button>
          <span style="font-size: 13px; color: #aaa">第 {{ page }} 页 ／ 共 {{ total }} 行</span>
          <n-button size="small" :disabled="!hasNextPage || querying" @click="nextPage">下一页</n-button>
        </div>
      </template>
    </n-card>

    <!-- 编辑／插入弹窗 -->
    <n-modal v-model:show="showForm" preset="card" :title="formTitle" style="width: 560px; max-width: 92vw">
      <n-form label-placement="left" label-width="auto">
        <n-form-item v-for="col in columns" :key="col.name" :label="col.name">
          <n-input
            v-model:value="formFields[col.name]"
            type="textarea"
            :autosize="{ minRows: 1, maxRows: 6 }"
            :disabled="formMode === 'edit' && pk.includes(col.name)"
            :placeholder="formPlaceholder(col)"
          />
          <template #feedback>
            <span style="font-size: 12px; color: #888">{{ col.data_type }}{{ col.nullable ? '（可空）' : '' }}</span>
          </template>
        </n-form-item>
      </n-form>
      <template #footer>
        <div style="display: flex; justify-content: flex-end; gap: 8px">
          <n-button @click="showForm = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="saveForm">保存</n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, h, watch } from 'vue'
import {
  NCard,
  NList,
  NListItem,
  NDataTable,
  NButton,
  NSpin,
  NEmpty,
  NInput,
  NSelect,
  NAlert,
  NModal,
  NForm,
  NFormItem,
  NPopconfirm,
  useMessage,
} from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import { store, send } from '../ws'
import { getAuthority } from '../auth'

type Row = Record<string, unknown>

interface TableMeta {
  name: string
  rows: number
}
interface DatabaseData {
  tables: TableMeta[]
}
interface ColumnMeta {
  name: string
  data_type: string
  nullable: boolean
}
interface FilterRow {
  column: string | null
  op: string
  value: string
}
interface QueryResult {
  columns: ColumnMeta[]
  pk: string[]
  rows: Row[]
  total: number
  limit: number
  offset: number
}

// 算子白名单，与后端一致。
const OPS = ['=', '!=', '<', '>', '<=', '>=', 'like', 'ilike', 'is null', 'is not null']
const NO_VALUE_OPS = ['is null', 'is not null']

const message = useMessage()
const authority = getAuthority()

const dbData = computed(() => store['database'] as DatabaseData | undefined)

const selectedTable = ref<string | null>(null)
const columns = ref<ColumnMeta[]>([])
const pk = ref<string[]>([])
const rows = ref<Row[]>([])
const total = ref(0)
const offset = ref(0)
const limit = ref(20)
const querying = ref(false)
const loaded = ref(false)

const searchInput = ref('')
const activeSearch = ref('')
const filters = ref<FilterRow[]>([])
const activeFilters = ref<FilterRow[]>([])
const orderBy = ref<string | null>(null)
const orderDir = ref<'asc' | 'desc'>('asc')

// 写权限：authority>=5。具体行是否可写还需该表有主键（见 canEditRows）。
const canWrite = computed(() => authority >= 5)
const canEditRows = computed(() => canWrite.value && pk.value.length > 0)

const page = computed(() => Math.floor(offset.value / limit.value) + 1)
const hasNextPage = computed(() => offset.value + rows.value.length < total.value)

const pageSizeOptions = [
  { label: '20 / 页', value: 20 },
  { label: '50 / 页', value: 50 },
  { label: '100 / 页', value: 100 },
]
const opOptions = OPS.map((o) => ({ label: o, value: o }))
const columnOptions = computed(() => columns.value.map((c) => ({ label: c.name, value: c.name })))

function opNeedsValue(op: string): boolean {
  return !NO_VALUE_OPS.includes(op)
}

// ISO-8601 日期时间(带 T 分隔)正则:抓到日期与到秒的时分秒,丢弃毫秒/微秒与时区。
const ISO_DT_RE = /^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2}:\d{2})(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?$/

// 把 ISO-8601 日期时间压成紧凑的「2026-06-05 15:05:04」;非该格式原样返回。
function fmtDateTime(s: string): string {
  const m = ISO_DT_RE.exec(s)
  return m ? `${m[1]} ${m[2]}` : s
}

// 单元格展示：对象／数组压成紧凑 JSON 文本;ISO 日期时间压成紧凑格式。
function cellText(v: unknown): string {
  if (v === null || v === undefined) return '—'
  if (typeof v === 'object') return JSON.stringify(v)
  if (typeof v === 'string') return fmtDateTime(v)
  return String(v)
}

// 行 key：有主键则用主键值拼，否则用整行 JSON（仅作 vue key，不参与写）。
function rowKey(r: Row): string {
  if (pk.value.length) return pk.value.map((k) => String(r[k])).join('')
  return JSON.stringify(r)
}

const gridColumns = computed<DataTableColumns<Row>>(() => {
  const cols: DataTableColumns<Row> = columns.value.map((col) => ({
    title: () => {
      const arrow = orderBy.value === col.name ? (orderDir.value === 'asc' ? ' ↑' : ' ↓') : ''
      return h(
        'span',
        { style: 'cursor:pointer;user-select:none', onClick: () => toggleSort(col.name) },
        `${col.name}${arrow}`,
      )
    },
    key: col.name,
    ellipsis: { tooltip: true },
    render: (row: Row) => cellText(row[col.name]),
  }))
  if (canEditRows.value) {
    cols.push({
      title: '操作',
      key: '__actions__',
      width: 130,
      render: (row: Row) =>
        h('div', { style: 'display:flex;gap:6px' }, [
          h(NButton, { size: 'tiny', onClick: () => openEdit(row) }, { default: () => '编辑' }),
          h(
            NPopconfirm,
            { onPositiveClick: () => doDelete(row), positiveText: '确定', negativeText: '取消' },
            {
              trigger: () => h(NButton, { size: 'tiny', type: 'error' }, { default: () => '删除' }),
              default: () => '确定删除该行？',
            },
          ),
        ]),
    })
  }
  return cols
})

async function doQuery() {
  if (!selectedTable.value) return
  querying.value = true
  try {
    const args: Record<string, unknown> = {
      table: selectedTable.value,
      limit: limit.value,
      offset: offset.value,
    }
    if (activeSearch.value) args.search = activeSearch.value
    if (orderBy.value) {
      args.order_by = orderBy.value
      args.order_dir = orderDir.value
    }
    const fs = activeFilters.value
      .filter((f) => f.column && OPS.includes(f.op))
      .map((f) => {
        const out: Record<string, unknown> = { column: f.column, op: f.op }
        if (opNeedsValue(f.op)) out.value = f.value
        return out
      })
    if (fs.length) args.filters = fs

    const res = (await send('db/query', args)) as QueryResult
    columns.value = res.columns ?? []
    pk.value = res.pk ?? []
    rows.value = res.rows ?? []
    total.value = res.total ?? 0
    loaded.value = true
  } catch (e) {
    rows.value = []
    total.value = 0
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    querying.value = false
  }
}

function selectTable(name: string) {
  if (selectedTable.value === name) return
  selectedTable.value = name
  // 切表清空所有查询态。
  offset.value = 0
  searchInput.value = ''
  activeSearch.value = ''
  filters.value = []
  activeFilters.value = []
  orderBy.value = null
  orderDir.value = 'asc'
  loaded.value = false
  columns.value = []
  pk.value = []
  rows.value = []
  doQuery()
}

function applySearch() {
  activeSearch.value = searchInput.value.trim()
  offset.value = 0
  doQuery()
}

function addFilterRow() {
  filters.value.push({ column: columns.value[0]?.name ?? null, op: '=', value: '' })
}
function removeFilterRow(i: number) {
  filters.value.splice(i, 1)
}
function clearFilters() {
  filters.value = []
  activeFilters.value = []
  offset.value = 0
  doQuery()
}
function applyFilters() {
  // 深拷贝一份作为生效态，避免后续编辑构建器影响已发出的查询。
  activeFilters.value = filters.value.map((f) => ({ ...f }))
  offset.value = 0
  doQuery()
}

function toggleSort(col: string) {
  if (orderBy.value === col) {
    orderDir.value = orderDir.value === 'asc' ? 'desc' : 'asc'
  } else {
    orderBy.value = col
    orderDir.value = 'asc'
  }
  offset.value = 0
  doQuery()
}

function onPageSize() {
  offset.value = 0
  doQuery()
}
function prevPage() {
  if (offset.value === 0) return
  offset.value = Math.max(0, offset.value - limit.value)
  doQuery()
}
function nextPage() {
  if (!hasNextPage.value) return
  offset.value += limit.value
  doQuery()
}

// ─── 增／改弹窗 ───
const showForm = ref(false)
const formMode = ref<'insert' | 'edit'>('insert')
const formFields = ref<Record<string, string>>({})
const editOriginal = ref<Row | null>(null)
const saving = ref(false)
const formTitle = computed(() => (formMode.value === 'insert' ? '插入新行' : '编辑行'))

function formPlaceholder(col: ColumnMeta): string {
  return col.nullable ? '留空写 NULL' : ''
}

// 把单元格值转成可编辑文本：对象／数组压成 JSON，null 留空。
function toFieldText(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'object') return JSON.stringify(v)
  return String(v)
}

function openInsert() {
  formMode.value = 'insert'
  editOriginal.value = null
  const f: Record<string, string> = {}
  for (const c of columns.value) f[c.name] = ''
  formFields.value = f
  showForm.value = true
}

function openEdit(row: Row) {
  formMode.value = 'edit'
  editOriginal.value = row
  const f: Record<string, string> = {}
  for (const c of columns.value) f[c.name] = toFieldText(row[c.name])
  formFields.value = f
  showForm.value = true
}

// 文本 → 写入值：空串映射为 null（写 SQL NULL）。其余原样为字符串，由后端 cast 到列类型。
function fieldToValue(text: string): unknown {
  if (text === '') return null
  return text
}

async function saveForm() {
  if (!selectedTable.value) return
  saving.value = true
  try {
    if (formMode.value === 'insert') {
      const values: Record<string, unknown> = {}
      for (const c of columns.value) {
        const t = formFields.value[c.name] ?? ''
        // 插入：留空的列直接不带（让数据库走默认值／序列）。
        if (t !== '') values[c.name] = fieldToValue(t)
      }
      await send('db/insert', { table: selectedTable.value, values })
      message.success('已插入')
    } else {
      const orig = editOriginal.value!
      // 只发改动过的非主键列。
      const set: Record<string, unknown> = {}
      for (const c of columns.value) {
        if (pk.value.includes(c.name)) continue
        const newText = formFields.value[c.name] ?? ''
        if (newText !== toFieldText(orig[c.name])) set[c.name] = fieldToValue(newText)
      }
      if (Object.keys(set).length === 0) {
        message.info('没有改动')
        saving.value = false
        return
      }
      const pkVals: Record<string, unknown> = {}
      for (const k of pk.value) pkVals[k] = orig[k]
      await send('db/update', { table: selectedTable.value, pk: pkVals, set })
      message.success('已更新')
    }
    showForm.value = false
    await doQuery()
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  } finally {
    saving.value = false
  }
}

async function doDelete(row: Row) {
  if (!selectedTable.value || pk.value.length === 0) return
  const pkVals: Record<string, unknown> = {}
  for (const k of pk.value) pkVals[k] = row[k]
  try {
    await send('db/delete', { table: selectedTable.value, pk: pkVals })
    message.success('已删除')
    await doQuery()
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  }
}

// store.database 刷新后，若当前选中的表已不存在则清空选择。
watch(dbData, (val) => {
  if (!val || !selectedTable.value) return
  if (!val.tables.some((t) => t.name === selectedTable.value)) {
    selectedTable.value = null
    rows.value = []
  }
})
</script>
