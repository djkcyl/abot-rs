<template>
  <div style="padding: 24px">
    <h2 style="margin-bottom: 24px">配置</h2>

    <n-spin v-if="!configData" size="medium" style="display: block; text-align: center; padding: 40px" />

    <template v-else-if="configData.specs.length === 0">
      <div style="text-align: center; color: #666; padding: 40px">暂无可配置的插件</div>
    </template>

    <div v-else style="display: flex; flex-direction: column; gap: 20px">
      <n-card v-for="spec in configData.specs" :key="spec.plugin_key">
        <template #header>{{ spec.title }}</template>

        <n-form :label-width="160" label-placement="left">
          <n-form-item
            v-for="[fieldKey, fieldSchema] in Object.entries(spec.schema.properties ?? {})"
            :key="fieldKey"
            :label="fieldSchema.description || fieldKey"
          >
            <n-switch
              v-if="fieldSchema.type === 'boolean'"
              :value="Boolean(formStates[spec.plugin_key]?.[fieldKey])"
              @update:value="(v) => setField(spec.plugin_key, fieldKey, v)"
            />
            <n-input-number
              v-else-if="fieldSchema.type === 'integer' || fieldSchema.type === 'number'"
              :value="formStates[spec.plugin_key]?.[fieldKey] as number | null"
              :precision="fieldSchema.type === 'integer' ? 0 : undefined"
              style="width: 100%"
              @update:value="(v) => setField(spec.plugin_key, fieldKey, v)"
            />
            <n-input
              v-else
              :value="String(formStates[spec.plugin_key]?.[fieldKey] ?? '')"
              style="width: 100%"
              @update:value="(v) => setField(spec.plugin_key, fieldKey, v)"
            />
          </n-form-item>
        </n-form>

        <div style="display: flex; align-items: center; gap: 12px; margin-top: 8px">
          <n-button
            type="primary"
            :loading="saving[spec.plugin_key]"
            @click="saveConfig(spec.plugin_key)"
          >
            保存
          </n-button>
          <span v-if="saveErrors[spec.plugin_key]" style="color: #e88080; font-size: 13px">
            {{ saveErrors[spec.plugin_key] }}
          </span>
        </div>
      </n-card>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { NCard, NForm, NFormItem, NInput, NInputNumber, NSwitch, NButton, NSpin, useMessage } from 'naive-ui'
import { store, send } from '../ws'

interface FieldSchema {
  type: 'string' | 'integer' | 'number' | 'boolean'
  description?: string
}

interface JsonSchema {
  properties?: Record<string, FieldSchema>
  required?: string[]
}

interface ConfigSpec {
  plugin_key: string
  title: string
  schema: JsonSchema
}

interface ConfigData {
  specs: ConfigSpec[]
  values: Record<string, Record<string, unknown>>
}

const message = useMessage()

const configData = computed(() => store['config'] as ConfigData | undefined)

// plugin_key → { fieldKey: value }
const formStates = reactive<Record<string, Record<string, unknown>>>({})
// plugin_key → saving flag
const saving = reactive<Record<string, boolean>>({})
// plugin_key → error string
const saveErrors = reactive<Record<string, string>>({})

// 当 configData 首次出现或更新时，初始化表单状态
watch(
  configData,
  (data) => {
    if (!data) return
    for (const spec of data.specs) {
      // 只在首次初始化，避免覆盖用户正在编辑的内容
      if (!(spec.plugin_key in formStates)) {
        formStates[spec.plugin_key] = { ...(data.values[spec.plugin_key] ?? {}) }
      }
    }
  },
  { immediate: true },
)

function setField(pluginKey: string, fieldKey: string, value: unknown) {
  if (!formStates[pluginKey]) {
    formStates[pluginKey] = {}
  }
  formStates[pluginKey][fieldKey] = value
}

async function saveConfig(pluginKey: string) {
  saving[pluginKey] = true
  saveErrors[pluginKey] = ''
  try {
    await send('config/set', { plugin_key: pluginKey, value: formStates[pluginKey] ?? {} })
    message.success('保存成功')
  } catch (e) {
    const msg = typeof e === 'string' ? e : String(e)
    saveErrors[pluginKey] = msg
    message.error(msg)
  } finally {
    saving[pluginKey] = false
  }
}
</script>
