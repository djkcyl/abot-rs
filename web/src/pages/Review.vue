<template>
  <div style="padding: 24px">
    <h2 style="margin-bottom: 24px">审核</h2>

    <n-spin v-if="!reviewData" size="medium" style="display: block; text-align: center; padding: 40px" />

    <template v-else-if="reviewData.sources.length === 0">
      <div style="text-align: center; color: #666; padding: 40px">暂无待审项</div>
    </template>

    <template v-else>
      <div
        v-for="source in reviewData.sources"
        :key="source.source"
        style="margin-bottom: 32px"
      >
        <h3 style="margin-bottom: 12px">{{ source.label }}</h3>
        <n-data-table
          :columns="buildColumns(source)"
          :data="itemsFor(source.source)"
          size="small"
          :row-props="(row) => ({ style: 'cursor:pointer', onClick: () => openDetail(source.source, (row as ReviewItem).id) })"
        />
      </div>
    </template>

    <!-- 详情 Modal -->
    <n-modal v-model:show="detailVisible" preset="card" title="详情" style="max-width: 640px; width: 90vw">
      <n-spin v-if="detailLoading" style="display: block; text-align: center; padding: 32px" />
      <div v-else-if="detailError" style="color: #e88080">{{ detailError }}</div>
      <div v-else-if="detailData">
        <div
          v-for="[k, v] in Object.entries(detailData)"
          :key="k"
          style="display: flex; gap: 12px; margin-bottom: 10px; align-items: flex-start"
        >
          <div style="min-width: 100px; font-weight: 500; color: #aaa; flex-shrink: 0">{{ k }}</div>
          <div style="flex: 1; word-break: break-all">
            <div v-if="imageList(k, v).length" style="display: flex; flex-wrap: wrap; gap: 8px">
              <a
                v-for="(url, i) in imageList(k, v)"
                :key="i"
                :href="url"
                target="_blank"
                rel="noopener"
              >
                <img
                  :src="url"
                  style="max-width: 200px; max-height: 200px; object-fit: contain; border-radius: 4px"
                />
              </a>
            </div>
            <img
              v-else-if="isImageUrl(v)"
              :src="String(v)"
              style="max-width: 100%; max-height: 240px; border-radius: 4px"
            />
            <span v-else>{{ typeof v === 'string' ? v : JSON.stringify(v) }}</span>
          </div>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, h } from 'vue'
import { NDataTable, NButton, NModal, NSpin, useMessage } from 'naive-ui'
import type { DataTableColumns, ButtonProps } from 'naive-ui'
import { store, send } from '../ws'

interface ColumnDef {
  key: string
  label: string
}

interface ActionDef {
  key: string
  label: string
  style: string
}

interface ReviewSource {
  source: string
  label: string
  columns: ColumnDef[]
  actions: ActionDef[]
}

interface ReviewItem {
  source: string
  id: string
  columns: Record<string, unknown>
}

interface ReviewData {
  sources: ReviewSource[]
  items: ReviewItem[]
}

const message = useMessage()

const reviewData = computed(() => store['review'] as ReviewData | undefined)

// 本地隐藏的条目 id（乐观删除）
const hiddenIds = ref<Set<string>>(new Set())

function itemsFor(source: string): ReviewItem[] {
  return (reviewData.value?.items ?? []).filter(
    (item) => item.source === source && !hiddenIds.value.has(item.id),
  )
}

function styleToType(style: string): ButtonProps['type'] {
  const map: Record<string, ButtonProps['type']> = {
    primary: 'primary',
    error: 'error',
    warning: 'warning',
    info: 'info',
    default: 'default',
  }
  return map[style] ?? 'default'
}

function buildColumns(source: ReviewSource): DataTableColumns<ReviewItem> {
  const dataCols: DataTableColumns<ReviewItem> = source.columns.map((col) => ({
    title: col.label,
    key: col.key,
    ellipsis: { tooltip: true },
    render(row: ReviewItem) {
      const val = row.columns[col.key]
      if (val === null || val === undefined) return '—'
      return String(val)
    },
  }))

  const actionCol = {
    title: '操作',
    key: '__actions__',
    width: source.actions.length * 80 + 16,
    render(row: ReviewItem) {
      return h(
        'div',
        { style: 'display:flex;gap:6px;', onClick: (e: MouseEvent) => e.stopPropagation() },
        source.actions.map((action) =>
          h(
            NButton,
            {
              size: 'small',
              type: styleToType(action.style),
              onClick: () => invokeAction(source.source, row.id, action.key),
            },
            { default: () => action.label },
          ),
        ),
      )
    },
  }

  return [...dataCols, actionCol]
}

async function invokeAction(source: string, id: string, action: string) {
  try {
    await send('review/invoke', { source, id, action })
    hiddenIds.value = new Set([...hiddenIds.value, id])
    message.success('操作成功')
  } catch (e) {
    message.error(typeof e === 'string' ? e : String(e))
  }
}

// --- 详情 Modal ---
const detailVisible = ref(false)
const detailLoading = ref(false)
const detailError = ref<string | null>(null)
const detailData = ref<Record<string, unknown> | null>(null)

async function openDetail(source: string, id: string) {
  detailVisible.value = true
  detailLoading.value = true
  detailError.value = null
  detailData.value = null
  try {
    const result = await send('review/detail', { source, id })
    detailData.value = result as Record<string, unknown>
  } catch (e) {
    detailError.value = typeof e === 'string' ? e : String(e)
  } finally {
    detailLoading.value = false
  }
}

// detail 里的 `images` 字段是一组图片 URL 字符串（如 /api/media/xxx），渲染成缩略图行；
// 其它字段返回空数组、走原有渲染。
function imageList(key: string, val: unknown): string[] {
  if (key !== 'images' || !Array.isArray(val)) return []
  return val.filter((u): u is string => typeof u === 'string' && u.length > 0)
}

function isImageUrl(val: unknown): boolean {
  if (typeof val !== 'string') return false
  if (!val.startsWith('http')) return false
  return /\.(png|jpe?g|gif|webp|svg|bmp)(\?.*)?$/i.test(val)
}
</script>
