<template>
  <div v-if="visible" class="ns-overlay">
    <div class="ns-card" role="dialog" aria-modal="true" aria-label="新建会话">
      <h2 class="title">新建会话</h2>
      <p class="subtitle">选择设备上的项目目录（或手动输入路径），会话将在新标签页打开。</p>
      <form @submit.prevent="onSubmit">
        <label>
          <span class="lbl">项目目录</span>
          <input
            ref="dirInput"
            v-model="directory"
            type="text"
            list="ns-project-list"
            placeholder="如 ~/projects/foo"
            :readonly="lockDirectory"
            required
          />
          <datalist id="ns-project-list">
            <option v-for="p in projects" :key="p.path" :value="p.path" />
          </datalist>
        </label>
        <p v-if="lockDirectory" class="muted lock-hint">
          路径已锁定为当前目录，确认后将在此目录新建会话。
        </p>

        <div v-if="loadingProjects" class="muted small">加载设备项目列表…</div>
        <div v-else-if="projects.length && !lockDirectory" class="quick">
          <button
            v-for="p in quickProjects"
            :key="p.path"
            type="button"
            class="chip"
            :class="{ active: directory === p.path }"
            @click="directory = p.path"
          >
            {{ p.path }}
          </button>
        </div>

        <p v-if="error" class="error">{{ error }}</p>
        <div class="actions">
          <button type="button" class="ghost" :disabled="busy" @click="onCancel">取消</button>
          <button type="submit" class="primary" :disabled="busy || !directory.trim()">
            {{ busy ? '创建中…' : '创建并打开' }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import type { ProjectInfo } from '../api'

const props = defineProps<{
  visible: boolean
  projects: ProjectInfo[]
  loadingProjects: boolean
  defaultDirectory: string
  busy: boolean
  error: string
  /** true = 路径锁定为 defaultDirectory（"新建会话"二次确认模式）：
      输入框只读、隐藏项目 chips；展示与选择模式完全一致。 */
  lockDirectory?: boolean
}>()

const emit = defineEmits<{
  submit: [directory: string]
  cancel: []
}>()

const directory = ref('')
const dirInput = ref<HTMLInputElement | null>(null)

const quickProjects = computed(() => props.projects.slice(0, 8))

watch(
  () => props.visible,
  async (v) => {
    if (v) {
      directory.value = props.defaultDirectory
      await nextTick()
      dirInput.value?.focus()
    }
  },
)

function onSubmit() {
  const dir = directory.value.trim()
  if (!dir) return
  emit('submit', dir)
}

function onCancel() {
  if (props.busy) return
  emit('cancel')
}
</script>

<style scoped>
.ns-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px 16px;
  z-index: 100;
}
.ns-card {
  width: 100%;
  max-width: 440px;
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px 20px;
  box-shadow: var(--shadow);
  max-height: 85vh;
  overflow-y: auto;
}
.title {
  margin: 0 0 4px;
  font-size: 1.2rem;
}
.subtitle {
  margin: 0 0 18px;
  color: var(--text-muted);
  font-size: 0.85rem;
}
form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.lbl {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text-muted);
}
.quick {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.chip {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 6px 12px;
  font-size: 0.8rem;
  font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
  color: var(--text);
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.chip:hover {
  border-color: var(--primary);
}
.chip.active {
  border-color: var(--primary);
  color: var(--primary);
  font-weight: 600;
}
.actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
  margin-top: 4px;
}
.small {
  font-size: 0.8rem;
}
.lock-hint {
  margin: -4px 0 0;
  font-size: 0.78rem;
}
input[readonly] {
  background: var(--bg);
  color: var(--text-muted);
}
</style>
