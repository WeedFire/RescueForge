<script setup lang="ts">
import { ref, computed } from 'vue'
import { FolderPlus, Trash2, Play, FolderOpen, X, Loader2, CornerDownLeft } from 'lucide-vue-next'
import { selectFolder, validatePath } from '@/api/tauri'

const props = defineProps<{
  isScanning: boolean
}>()

const emit = defineEmits<{
  startScan: [paths: string[]]
}>()

const paths = ref<string[]>([])
const adding = ref(false)
const manualInput = ref('')
const validating = ref(false)
const inputError = ref('')

function pushPath(p: string) {
  if (!paths.value.includes(p)) {
    paths.value.push(p)
  }
}

async function addPath() {
  adding.value = true
  inputError.value = ''
  try {
    const selected = await selectFolder()
    if (selected) pushPath(selected)
  } catch (e) {
    console.error('选择文件夹失败:', e)
  } finally {
    adding.value = false
  }
}

async function addManualPath() {
  const raw = manualInput.value.trim()
  if (!raw) return
  validating.value = true
  inputError.value = ''
  try {
    const result = await validatePath(raw)
    if (result.kind === 'missing') {
      inputError.value = `路径不存在或不可访问: ${result.normalized}`
      return
    }
    if (paths.value.includes(result.normalized)) {
      inputError.value = '该路径已在列表中'
      return
    }
    pushPath(result.normalized)
    manualInput.value = ''
  } catch (e) {
    inputError.value = String(e)
  } finally {
    validating.value = false
  }
}

function removePath(index: number) {
  paths.value.splice(index, 1)
}

function startScan() {
  if (paths.value.length === 0) return
  emit('startScan', [...paths.value])
}

function clearAll() {
  paths.value = []
  inputError.value = ''
}

const hasPaths = computed(() => paths.value.length > 0)
</script>

<template>
  <section class="card">
    <!-- Header -->
    <div class="flex items-center justify-between px-5 py-4 border-b border-app-border">
      <div class="flex items-center gap-2.5">
        <div class="w-8 h-8 rounded-lg bg-brand-cyan/10 flex items-center justify-center">
          <FolderOpen class="w-4 h-4 text-brand-cyan" />
        </div>
        <div>
          <h2 class="text-sm font-semibold">文件夹扫描</h2>
          <p class="text-3xs text-t-muted">选择文件夹路径，扫描并恢复丢失的文件</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <button
          v-if="hasPaths"
          @click="clearAll"
          class="btn-ghost-sm text-t-muted hover:text-t-secondary"
          :disabled="props.isScanning"
        >
          <X class="w-3 h-3" />
          清空
        </button>
        <button
          @click="addPath"
          class="btn-secondary-sm gap-1.5"
          :disabled="props.isScanning || adding"
        >
          <Loader2 v-if="adding" class="w-3.5 h-3.5 animate-spin-slow" />
          <FolderPlus v-else class="w-3.5 h-3.5" />
          添加文件夹
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="p-5">
      <!-- Manual path input -->
      <div class="mb-4">
        <div class="flex gap-2">
          <div class="relative flex-1">
            <label for="manual-path-input" class="sr-only">手动输入扫描路径</label>
            <input
              id="manual-path-input"
              v-model="manualInput"
              type="text"
              placeholder="手动输入任意路径，如 D:\temp 或 D:\照片\img.jpg"
              aria-label="手动输入扫描路径"
              :aria-invalid="!!inputError"
              class="w-full px-3 py-2 pr-9 rounded-md bg-app-input border border-app-border text-sm text-t-primary font-mono placeholder:text-t-muted placeholder:font-sans focus:outline-none focus:border-brand-blue/50 transition-colors"
              :class="inputError && 'border-brand-red/50'"
              :disabled="props.isScanning"
              @keydown.enter="addManualPath"
            />
            <CornerDownLeft class="absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-t-muted pointer-events-none" />
          </div>
          <button
            @click="addManualPath"
            class="btn-secondary-sm gap-1.5 flex-shrink-0"
            :disabled="props.isScanning || validating || !manualInput.trim()"
          >
            <Loader2 v-if="validating" class="w-3.5 h-3.5 animate-spin-slow" />
            添加路径
          </button>
        </div>
        <p v-if="inputError" class="text-xs text-brand-red mt-1.5" role="alert">{{ inputError }}</p>
        <p v-else class="text-2xs text-t-muted mt-1.5">支持文件夹或单个文件路径，回车快速添加</p>
      </div>

      <!-- Empty state -->
      <div v-if="!hasPaths" class="text-center py-8">
        <div class="w-14 h-14 rounded-2xl bg-app-input flex items-center justify-center mx-auto mb-4">
          <FolderOpen class="w-7 h-7 text-t-muted" />
        </div>
        <h3 class="text-sm font-medium mb-1">未添加扫描路径</h3>
        <p class="text-xs text-t-muted max-w-xs mx-auto mb-4">
          在上方输入框手动输入任意路径（如 D:\temp），或点击"添加文件夹"按钮选择。支持多个路径同时扫描。
        </p>
        <button
          @click="addPath"
          class="btn-primary-sm gap-1.5"
          :disabled="adding"
        >
          <FolderPlus class="w-3.5 h-3.5" />
          添加第一个文件夹
        </button>
      </div>

      <!-- Path list -->
      <div v-else class="space-y-2">
        <div
          v-for="(path, index) in paths"
          :key="path"
          class="flex items-center gap-3 px-3 py-2.5 rounded-md bg-app-input border border-app-border group hover:border-brand-blue/30 transition-colors"
        >
          <div class="w-7 h-7 rounded-md bg-brand-blue/10 flex items-center justify-center flex-shrink-0">
            <span class="text-2xs font-semibold text-brand-blue">{{ index + 1 }}</span>
          </div>
          <span class="text-sm text-t-primary truncate flex-1 font-mono text-xs">{{ path }}</span>
          <button
            @click="removePath(index)"
            class="p-1.5 rounded opacity-0 group-hover:opacity-100 hover:bg-brand-red/10 text-t-muted hover:text-brand-red transition-all"
            :disabled="props.isScanning"
          >
            <Trash2 class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <!-- Start scan button -->
      <div v-if="hasPaths" class="mt-4 pt-4 border-t border-app-border">
        <button
          @click="startScan"
          class="btn-primary w-full gap-2"
          :disabled="props.isScanning"
        >
          <Play class="w-4 h-4" />
          {{ props.isScanning ? '扫描进行中...' : `开始扫描 ${paths.length} 个路径` }}
        </button>
      </div>
    </div>
  </section>
</template>
