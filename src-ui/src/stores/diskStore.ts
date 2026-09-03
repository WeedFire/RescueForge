import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import type { DiskInfo, SmartStatus } from '@/types'

export const useDiskStore = defineStore('disk', () => {
  const disks = ref<DiskInfo[]>([])
  const selectedDiskIndex = ref<number | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  const selectedDisk = computed(() => {
    if (selectedDiskIndex.value === null) return null
    return disks.value.find(d => d.index === selectedDiskIndex.value) ?? null
  })

  async function fetchDisks() {
    loading.value = true
    error.value = null
    try {
      const { getDisks } = await import('@/api/tauri')
      disks.value = await getDisks()
    } catch (e) {
      error.value = `获取磁盘列表失败: ${e}`
      console.error(e)
    } finally {
      loading.value = false
    }
  }

  async function fetchSmartStatus(diskIndex: number): Promise<SmartStatus | null> {
    try {
      const { getSmartStatus } = await import('@/api/tauri')
      const status = await getSmartStatus(diskIndex)
      const disk = disks.value.find(d => d.index === diskIndex)
      if (disk) {
        disk.smart_status = status
      }
      return status
    } catch (e) {
      console.error(e)
      return null
    }
  }

  function selectDisk(index: number) {
    selectedDiskIndex.value = index
  }

  return {
    disks,
    selectedDiskIndex,
    selectedDisk,
    loading,
    error,
    fetchDisks,
    fetchSmartStatus,
    selectDisk,
  }
})
