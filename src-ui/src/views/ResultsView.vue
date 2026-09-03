<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useScanStore } from '@/stores/scanStore'
import { recoverFiles, previewFile, selectFolder, revealInExplorer } from '@/api/tauri'
import {
  FolderOpen, Image, FileText, Film, Music, Archive, File, Download,
  Eye, CheckSquare, Square, Search, ChevronUp, ChevronDown, X, Trash2, ShieldAlert
} from 'lucide-vue-next'

const { t } = useI18n()
const scanStore = useScanStore()

// State
const selectedIds = ref<Set<string>>(new Set())
const previewing = ref<string | null>(null)
const previewData = ref<Uint8Array | null>(null)
const previewType = ref<'image' | 'text' | 'binary' | 'unknown'>('unknown')
const previewError = ref('')
const recovering = ref(false)
const recoverOutput = ref<string>('')
const recoverProgress = ref('')
const recoverStatus = ref<'success' | 'error'>('success')
const filterType = ref('all')
const searchQuery = ref('')
const sortBy = ref<'name' | 'size' | 'type'>('name')
const sortOrder = ref<'asc' | 'desc'>('asc')
// 分页渲染：结果可能达数万条，一次渲染全部行会卡死 UI，每屏先渲染 500 行，按需加载。
const PAGE_ROWS = 500
const visibleCount = ref(PAGE_ROWS)

// Get files from scan store
const allFiles = computed(() => scanStore.discoveredFiles)

const filteredFiles = computed(() => {
  let result = [...allFiles.value]

  // Filter by type
  if (filterType.value === 'deleted') {
    result = result.filter(f => f.deleted === true)
  } else if (filterType.value !== 'all') {
    const typeMap: Record<string, string[]> = {
      images: ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp', 'ico', 'svg', 'psd'],
      documents: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'md', 'csv', 'rtf', 'html', 'xml', 'json'],
      videos: ['mp4', 'avi', 'mkv', 'mov', 'wmv', 'flv', 'webm'],
      audio: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'wma'],
      archives: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2'],
    }
    const exts = typeMap[filterType.value] ?? []
    if (exts.length > 0) {
      result = result.filter(f => exts.includes(f.extension.toLowerCase()))
    }
  }

  // Search
  if (searchQuery.value) {
    const q = searchQuery.value.toLowerCase()
    result = result.filter(f => f.filename.toLowerCase().includes(q))
  }

  // Sort（数万条时避免 localeCompare 的多语言排序开销，用简单字符串比较）
  result.sort((a, b) => {
    let cmp = 0
    if (sortBy.value === 'name') cmp = a.filename < b.filename ? -1 : a.filename > b.filename ? 1 : 0
    else if (sortBy.value === 'size') cmp = a.size_bytes - b.size_bytes
    else if (sortBy.value === 'type') cmp = a.file_type < b.file_type ? -1 : a.file_type > b.file_type ? 1 : 0
    return sortOrder.value === 'asc' ? cmp : -cmp
  })

  return result
})

// 当前屏实际渲染的行（切片后的过滤结果）
const visibleFiles = computed(() => filteredFiles.value.slice(0, visibleCount.value))
const hasMore = computed(() => filteredFiles.value.length > visibleCount.value)

function loadMore() {
  visibleCount.value += PAGE_ROWS
}

// 过滤/搜索/排序变化时回到首屏，避免切片越界或停留在无效位置
watch([filterType, searchQuery, sortBy, sortOrder], () => {
  visibleCount.value = PAGE_ROWS
})

const totalSize = computed(() => {
  return filteredFiles.value.reduce((sum, f) => sum + f.size_bytes, 0)
})

const categories = [
  { key: 'all', icon: FolderOpen, label: '全部' },
  { key: 'deleted', icon: Trash2, label: '已删除' },
  { key: 'images', icon: Image, label: '图片' },
  { key: 'documents', icon: FileText, label: '文档' },
  { key: 'videos', icon: Film, label: '视频' },
  { key: 'audio', icon: Music, label: '音频' },
  { key: 'archives', icon: Archive, label: '压缩包' },
]

function toggleSelect(id: string) {
  const s = new Set(selectedIds.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selectedIds.value = s
}

function selectAll() {
  // 仅针对当前已加载的行做全选，避免一次勾选数万条导致巨型 IPC 消息
  if (selectedIds.value.size === visibleFiles.value.length && visibleFiles.value.length > 0) {
    selectedIds.value = new Set()
  } else {
    selectedIds.value = new Set(visibleFiles.value.map(f => f.id))
  }
}

async function handlePreview(file: { filepath: string, extension: string }) {
  previewing.value = file.filepath
  previewError.value = ''
  previewData.value = null
  try {
    const data = await previewFile(file.filepath)
    previewData.value = new Uint8Array(data)
    const imgExts = ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp', 'ico']
    const textExts = ['txt', 'md', 'csv', 'html', 'xml', 'json', 'js', 'ts', 'py', 'rs', 'css', 'log']
    if (imgExts.includes(file.extension.toLowerCase())) {
      previewType.value = 'image'
    } else if (textExts.includes(file.extension.toLowerCase())) {
      previewType.value = 'text'
    } else {
      previewType.value = 'binary'
    }
  } catch (e) {
    previewError.value = `无法预览该文件: ${e}`
    previewType.value = 'unknown'
  }
}

function closePreview() {
  previewing.value = null
  previewData.value = null
  previewError.value = ''
}

// Preview rendering helpers (浏览器全局对象需在 script 中使用，vue-tsc 无法解析模板中的 URL/Blob/TextDecoder)
const previewImageUrl = computed(() => {
  if (previewType.value !== 'image' || !previewData.value) return ''
  return URL.createObjectURL(new Blob([previewData.value as BlobPart]))
})

const previewText = computed(() => {
  if (!previewData.value) return ''
  return new TextDecoder().decode(previewData.value).slice(0, 50000)
})

const previewHex = computed(() => {
  if (!previewData.value) return ''
  return Array.from(previewData.value.slice(0, 256))
    .map(b => b.toString(16).padStart(2, '0'))
    .join(' ')
})

async function handleRecover() {
  if (selectedIds.value.size === 0) return
  if (!recoverOutput.value) {
    const selected = await selectFolder()
    if (!selected) return
    recoverOutput.value = selected
  }

  recovering.value = true
  try {
    // Build file IDs with path encoding: id||filepath（用 Map 一次建索引，避免逐选择 O(n) 查找）
    const fileMap = new Map(allFiles.value.map(f => [f.id, f]))
    const ids = [...selectedIds.value].map(id => {
      const file = fileMap.get(id)
      if (!file) return id
      // 深度恢复来源附加原始文件名，供后端按原名落盘（回收站残留 $R 无有效文件名）
      const orig = file.original_path
        ? `||${file.original_path}`
        : ''
      return `${id}||${file.filepath}${orig}`
    })
    const result = await recoverFiles(ids, recoverOutput.value)
    recoverStatus.value = result.failed > 0 ? 'error' : 'success'
    recoverProgress.value = result.failed > 0
      ? `恢复完成: ${result.success} 成功, ${result.failed} 失败 → ${recoverOutput.value}`
      : `恢复完成: ${result.success} 个文件 → ${recoverOutput.value}`
    selectedIds.value = new Set()
  } catch (e: any) {
    recoverStatus.value = 'error'
    recoverProgress.value = `恢复失败: ${e}`
  } finally {
    recovering.value = false
  }
}

async function changeRecoverOutput() {
  const selected = await selectFolder()
  if (selected) {
    recoverOutput.value = selected
    recoverProgress.value = ''
  }
}

function dismissRecoverBanner() {
  recoverProgress.value = ''
}

async function openRecoverOutput() {
  if (!recoverOutput.value) return
  try {
    await revealInExplorer(recoverOutput.value)
  } catch (e) {
    console.error('打开目录失败:', e)
  }
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1073741824) return `${(bytes / 1048576).toFixed(1)} MB`
  return `${(bytes / 1073741824).toFixed(2)} GB`
}

function toggleSort(field: 'name' | 'size' | 'type') {
  if (sortBy.value === field) {
    sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
  } else {
    sortBy.value = field
    sortOrder.value = 'asc'
  }
}
</script>

<template>
  <div class="p-6 space-y-5 max-w-[1400px] mx-auto">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div>
        <h1 class="text-xl font-semibold tracking-tight">扫描结果</h1>
        <p class="text-sm text-t-secondary mt-0.5">
          {{ allFiles.length }} 个文件 · {{ formatSize(totalSize) }}
          <span v-if="scanStore.isScanning" class="text-brand-blue animate-pulse ml-2">扫描进行中...</span>
        </p>
      </div>
      <button
        v-if="selectedIds.size > 0"
        @click="handleRecover"
        class="btn-primary gap-2"
        :disabled="recovering"
      >
        <Download class="w-4 h-4" />
        {{ recovering ? '恢复中...' : `恢复选中 (${selectedIds.size})` }}
      </button>
    </div>

    <!-- 深度扫描降级警告：无管理员权限时强制删除文件扫不到，必须醒目提示 -->
    <div
      v-if="scanStore.scanWarnings.length > 0"
      class="flex items-start gap-3 px-4 py-3 rounded-lg bg-brand-amber/10 border border-brand-amber/25"
    >
      <ShieldAlert class="w-4 h-4 text-brand-amber flex-shrink-0 mt-0.5" />
      <div class="flex-1 min-w-0">
        <p class="text-sm font-medium text-t-primary">已删除文件未能完整扫描</p>
        <p class="text-xs text-t-secondary mt-1 leading-relaxed">
          当前未以管理员身份运行，无法直接读取磁盘卷，强制删除（Shift+Delete）、回收站已清空的文件无法恢复。
          请以管理员身份重启本程序后重新扫描。
        </p>
      </div>
    </div>


    <!-- Recover output dir indicator -->
    <div v-if="recoverOutput && !recoverProgress" class="flex items-center gap-2 text-xs text-t-muted">
      <FolderOpen class="w-3.5 h-3.5" />
      <span>输出目录: <span class="font-mono text-t-secondary">{{ recoverOutput }}</span></span>
      <button @click="changeRecoverOutput" class="text-brand-blue hover:underline">更换</button>
    </div>

    <!-- Recover progress -->
    <div
      v-if="recoverProgress"
      class="card p-3 border"
      :class="recoverStatus === 'error' ? 'bg-brand-red/5 border-brand-red/20' : 'bg-brand-green/5 border-brand-green/20'"
    >
      <div class="flex items-center gap-3">
        <p
          class="text-sm flex-1"
          :class="recoverStatus === 'error' ? 'text-brand-red' : 'text-brand-green'"
          role="status"
        >
          {{ recoverProgress }}
        </p>
        <button
          v-if="recoverStatus === 'success' && recoverOutput"
          @click="openRecoverOutput"
          class="btn-ghost-sm gap-1 flex-shrink-0"
        >
          <FolderOpen class="w-3.5 h-3.5" />
          打开目录
        </button>
        <button
          @click="dismissRecoverBanner"
          class="p-1 rounded hover:bg-app-input text-t-muted hover:text-t-primary flex-shrink-0"
          aria-label="关闭提示"
        >
          <X class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Category Filters -->
    <div class="flex items-center gap-1.5 overflow-x-auto pb-1">
      <button
        v-for="cat in categories" :key="cat.key"
        @click="filterType = cat.key"
        :class="[
          'flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium whitespace-nowrap transition-colors',
          filterType === cat.key
            ? 'bg-brand-blue text-white'
            : 'text-t-secondary hover:text-t-primary bg-app-card hover:bg-app-card-hover'
        ]"
      >
        <component :is="cat.icon" class="w-3 h-3" />
        {{ cat.label }}
      </button>

      <!-- Search -->
      <div class="flex-1" />
      <div class="relative">
        <Search class="w-3.5 h-3.5 absolute left-2.5 top-1/2 -translate-y-1/2 text-t-muted" />
        <input
          v-model="searchQuery"
          placeholder="搜索文件名..."
          class="input pl-8 h-8 text-xs w-48"
        />
      </div>
    </div>

    <!-- Empty State -->
    <div v-if="allFiles.length === 0" class="card p-12 text-center">
      <div class="w-16 h-16 rounded-2xl bg-app-input flex items-center justify-center mx-auto mb-5">
        <FolderOpen class="w-8 h-8 text-t-muted" />
      </div>
      <h2 class="text-lg font-semibold mb-1">暂无扫描结果</h2>
      <p class="text-sm text-t-secondary max-w-sm mx-auto">
        请先在仪表盘启动一次文件夹扫描。扫描结果会实时显示在这里。
      </p>
    </div>

    <!-- File Table -->
    <div v-else class="card overflow-hidden">
      <!-- Table header -->
      <div class="grid grid-cols-[auto_1fr_100px_120px_100px] gap-3 px-4 py-2.5 bg-app-input border-b border-app-border text-2xs font-semibold text-t-muted uppercase tracking-wider">
        <div class="flex items-center gap-2">
          <button @click="selectAll" class="p-0.5 hover:text-brand-blue transition-colors">
            <CheckSquare v-if="selectedIds.size === visibleFiles.length && visibleFiles.length > 0" class="w-3.5 h-3.5 text-brand-blue" />
            <Square v-else class="w-3.5 h-3.5" />
          </button>
        </div>
        <button @click="toggleSort('name')" class="flex items-center gap-1 hover:text-t-primary transition-colors text-left">
          文件名
          <ChevronUp v-if="sortBy === 'name' && sortOrder === 'asc'" class="w-3 h-3" />
          <ChevronDown v-else-if="sortBy === 'name'" class="w-3 h-3" />
        </button>
        <button @click="toggleSort('type')" class="flex items-center gap-1 hover:text-t-primary transition-colors">
          类型
          <ChevronUp v-if="sortBy === 'type' && sortOrder === 'asc'" class="w-3 h-3" />
          <ChevronDown v-else-if="sortBy === 'type'" class="w-3 h-3" />
        </button>
        <button @click="toggleSort('size')" class="flex items-center gap-1 hover:text-t-primary transition-colors text-right">
          大小
          <ChevronUp v-if="sortBy === 'size' && sortOrder === 'asc'" class="w-3 h-3" />
          <ChevronDown v-else-if="sortBy === 'size'" class="w-3 h-3" />
        </button>
        <div class="text-right">操作</div>
      </div>

      <!-- File rows -->
      <div class="max-h-[60vh] overflow-y-auto">
        <div
          v-for="file in visibleFiles"
          :key="file.id"
          :class="[
            'grid grid-cols-[auto_1fr_100px_120px_100px] gap-3 px-4 py-2.5 border-b border-app-border/50 hover:bg-app-card-hover transition-colors text-sm',
            selectedIds.has(file.id) ? 'bg-brand-blue/5' : ''
          ]"
        >
          <!-- Checkbox -->
          <div class="flex items-center">
            <button @click="toggleSelect(file.id)" class="p-0.5">
              <CheckSquare v-if="selectedIds.has(file.id)" class="w-4 h-4 text-brand-blue" />
              <Square v-else class="w-4 h-4 text-t-muted hover:text-brand-blue" />
            </button>
          </div>

          <!-- Filename -->
          <div class="flex items-center gap-2 min-w-0">
            <Trash2 v-if="file.deleted" class="w-4 h-4 text-brand-red flex-shrink-0" />
            <File v-else class="w-4 h-4 text-t-muted flex-shrink-0" />
            <div class="min-w-0">
              <div class="flex items-center gap-2 min-w-0">
                <span class="truncate font-medium">{{ file.filename }}</span>
                <span
                  v-if="file.deleted"
                  class="flex-shrink-0 text-2xs px-1.5 py-0.5 rounded bg-brand-red/10 text-brand-red font-medium"
                  title="深度恢复发现：文件已被删除或剪切走，可从磁盘残留数据恢复"
                >
                  已删除
                </span>
                <span
                  v-if="file.data_intact === false"
                  class="flex-shrink-0 text-2xs px-1.5 py-0.5 rounded bg-brand-amber/15 text-brand-amber font-medium"
                  title="该文件的数据区已被新数据覆写（读到全零），恢复出来多半是空文件或无法打开"
                >
                  数据已覆盖
                </span>
              </div>
              <!-- 删除前完整路径：同名文件多次删除时据此区分各自原本所在目录 -->
              <div
                v-if="file.original_path"
                class="truncate text-2xs text-t-muted font-mono"
                :title="file.original_path"
              >
                {{ file.original_path }}
              </div>
            </div>
          </div>

          <!-- Type -->
          <div class="flex items-center">
            <span class="text-xs px-1.5 py-0.5 rounded bg-app-input text-t-secondary truncate">
              {{ file.file_type }}
            </span>
          </div>

          <!-- Size -->
          <div class="flex items-center justify-end tabular-nums text-t-secondary text-xs">
            {{ formatSize(file.size_bytes) }}
          </div>

          <!-- Actions -->
          <div class="flex items-center justify-end gap-1">
            <button
              v-if="!file.deleted"
              @click="handlePreview(file)"
              class="p-1.5 rounded hover:bg-app-card text-t-muted hover:text-brand-blue transition-colors"
              title="预览"
            >
              <Eye class="w-3.5 h-3.5" />
            </button>
            <span v-else class="p-1.5 text-2xs text-t-muted" title="已删除文件无法预览，可直接勾选恢复">恢复后可预览</span>
          </div>
        </div>
        <!-- Load more -->
        <div v-if="hasMore" class="p-3 text-center border-t border-app-border">
          <button @click="loadMore" class="btn-ghost-sm">
            加载更多（已显示 {{ visibleFiles.length }} / {{ filteredFiles.length }}）
          </button>
        </div>
      </div>
    </div>

    <!-- Preview Modal -->
    <div v-if="previewing" class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" @click.self="closePreview">
      <div class="bg-app-card border border-app-border rounded-xl shadow-2xl w-[800px] max-h-[80vh] flex flex-col overflow-hidden">
        <!-- Modal header -->
        <div class="flex items-center justify-between px-5 py-3 border-b border-app-border">
          <h3 class="text-sm font-semibold truncate">{{ previewing }}</h3>
          <button @click="closePreview" class="p-1 rounded hover:bg-app-input text-t-muted hover:text-t-primary">
            <X class="w-4 h-4" />
          </button>
        </div>
        <!-- Modal content -->
        <div class="flex-1 overflow-auto p-4">
          <!-- Preview error -->
          <div v-if="previewError" class="flex flex-col items-center justify-center py-12 gap-2">
            <File class="w-8 h-8 text-brand-red" />
            <p class="text-sm text-brand-red">{{ previewError }}</p>
          </div>
          <!-- Image preview -->
          <div v-else-if="previewType === 'image' && previewData" class="flex items-center justify-center">
            <img
              :src="previewImageUrl"
              class="max-w-full max-h-[60vh] object-contain rounded"
            />
          </div>
          <!-- Text preview -->
          <div v-else-if="previewType === 'text' && previewData" class="bg-app-input rounded p-4">
            <pre class="text-xs text-t-secondary font-mono whitespace-pre-wrap break-all max-h-[60vh] overflow-auto">{{ previewText }}</pre>
          </div>
          <!-- Binary preview -->
          <div v-else-if="previewType === 'binary' && previewData" class="bg-app-input rounded p-4">
            <p class="text-xs text-t-muted mb-2">二进制文件 (前 256 字节)</p>
            <pre class="text-xs text-t-secondary font-mono break-all">{{ previewHex }}</pre>
          </div>
          <!-- Loading -->
          <div v-else class="flex items-center justify-center py-12">
            <div class="w-8 h-8 border-2 border-app-border border-t-brand-blue rounded-full animate-spin-slow" />
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
