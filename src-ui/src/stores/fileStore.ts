import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { RecoveredFile, FileStatus } from '@/types'

export const useFileStore = defineStore('file', () => {
  const files = ref<RecoveredFile[]>([])
  const cartIds = ref<Set<string>>(new Set())
  const selectedFileId = ref<string | null>(null)
  const filterType = ref<string>('all')
  const viewMode = ref<'grid' | 'list'>('grid')
  const sortBy = ref<'name' | 'size' | 'time'>('name')
  const sortOrder = ref<'asc' | 'desc'>('asc')
  const searchQuery = ref('')

  const filteredFiles = computed(() => {
    let result = [...files.value]

    if (filterType.value !== 'all') {
      const typeMap: Record<string, string[]> = {
        images: ['jpg', 'jpeg', 'png', 'gif', 'bmp', 'webp', 'svg', 'ico'],
        documents: ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'txt', 'md', 'csv'],
        videos: ['mp4', 'avi', 'mkv', 'mov', 'wmv', 'flv', 'webm'],
        audio: ['mp3', 'wav', 'flac', 'aac', 'ogg', 'wma'],
        archives: ['zip', 'rar', '7z', 'tar', 'gz', 'bz2'],
      }
      const extensions = typeMap[filterType.value] ?? []
      if (extensions.length > 0) {
        result = result.filter(f => extensions.includes(f.extension.toLowerCase()))
      }
    }

    if (searchQuery.value) {
      const query = searchQuery.value.toLowerCase()
      result = result.filter(f => f.filename.toLowerCase().includes(query))
    }

    result.sort((a, b) => {
      let cmp = 0
      if (sortBy.value === 'name') cmp = a.filename.localeCompare(b.filename)
      else if (sortBy.value === 'size') cmp = a.size_bytes - b.size_bytes
      else if (sortBy.value === 'time') {
        const timeA = a.modified_time ? new Date(a.modified_time).getTime() : 0
        const timeB = b.modified_time ? new Date(b.modified_time).getTime() : 0
        cmp = timeA - timeB
      }
      return sortOrder.value === 'asc' ? cmp : -cmp
    })

    return result
  })

  const selectedFile = computed(() => {
    if (!selectedFileId.value) return null
    return files.value.find(f => f.id === selectedFileId.value) ?? null
  })

  const cartFiles = computed(() => {
    return files.value.filter(f => cartIds.value.has(f.id))
  })

  const cartTotalSize = computed(() => {
    return cartFiles.value.reduce((sum, f) => sum + f.size_bytes, 0)
  })

  function addFiles(newFiles: RecoveredFile[]) {
    const existingIds = new Set(files.value.map(f => f.id))
    const unique = newFiles.filter(f => !existingIds.has(f.id))
    files.value.push(...unique)
  }

  function selectFile(id: string) {
    selectedFileId.value = id
  }

  function addToCart(id: string) {
    cartIds.value = new Set([...cartIds.value, id])
  }

  function removeFromCart(id: string) {
    const newSet = new Set(cartIds.value)
    newSet.delete(id)
    cartIds.value = newSet
  }

  function toggleCart(id: string) {
    if (cartIds.value.has(id)) {
      removeFromCart(id)
    } else {
      addToCart(id)
    }
  }

  function clearCart() {
    cartIds.value = new Set()
  }

  function setFilter(type: string) {
    filterType.value = type
  }

  function setViewMode(mode: 'grid' | 'list') {
    viewMode.value = mode
  }

  function setSort(field: 'name' | 'size' | 'time') {
    if (sortBy.value === field) {
      sortOrder.value = sortOrder.value === 'asc' ? 'desc' : 'asc'
    } else {
      sortBy.value = field
      sortOrder.value = 'asc'
    }
  }

  function setSearch(query: string) {
    searchQuery.value = query
  }

  return {
    files,
    cartIds,
    selectedFileId,
    filterType,
    viewMode,
    sortBy,
    sortOrder,
    searchQuery,
    filteredFiles,
    selectedFile,
    cartFiles,
    cartTotalSize,
    addFiles,
    selectFile,
    addToCart,
    removeFromCart,
    toggleCart,
    clearCart,
    setFilter,
    setViewMode,
    setSort,
    setSearch,
  }
})
