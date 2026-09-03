import { defineStore } from 'pinia'
import { ref, shallowRef, triggerRef, computed } from 'vue'
import type { ScanSession, ScanEvent, ScanStatus } from '@/types'

export interface FoundFileInfo {
  id: string
  filename: string
  filepath: string
  extension: string
  size_bytes: number
  file_type: string
  modified_time: string | null
  /** true = 深度恢复发现的已删除/已剪走文件（回收站或 MFT 残留） */
  deleted?: boolean
  /** 删除前的完整原始路径（如 D:\temp\sub\a.txt），仅深度恢复来源有值。
   *  同名文件被多次删除时，靠此字段区分它们原本所在的目录。 */
  original_path?: string | null
  /** false = 数据簇疑似已被覆写（扫描时读到全零），恢复出来多半是空文件/打不开 */
  data_intact?: boolean | null
}

export const useScanStore = defineStore('scan', () => {
  const sessions = ref<ScanSession[]>([])
  const currentSessionId = ref<string | null>(null)
  const isScanning = ref(false)
  const isPaused = ref(false)
  const scanLog = ref<string[]>([])
  // 深度扫描可能发现数万文件：用 shallowRef + 普通数组，
  // 避免 Vue 为每个文件对象建深度代理（8 万条会卡死主线程）。
  const discoveredFiles = shallowRef<FoundFileInfo[]>([])
  /** 后端降级提示（如卷无管理员权限 → 强制删除文件未能扫描）。
   *  必须在结果页醒目展示，否则用户会以为「扫不出来」是软件坏了。 */
  const scanWarnings = ref<string[]>([])
  // 非响应式去重集（每批事件 O(batch) 查重，不再整表重建）
  let discoveredIds = new Set<string>()

  function addDiscovered(files: FoundFileInfo[]) {
    if (!files || files.length === 0) return
    const arr = discoveredFiles.value
    let added = 0
    for (const f of files) {
      if (!discoveredIds.has(f.id)) {
        discoveredIds.add(f.id)
        arr.push(f)
        added++
      }
    }
    if (added > 0) triggerRef(discoveredFiles)
  }

  // 扫描完成后从后端分页拉取完整结果（事件只内联前 2000 条预览）
  async function fetchFullResults(sessionId: string) {
    try {
      const { getScanResults } = await import('@/api/tauri')
      const PAGE = 5000
      const all: FoundFileInfo[] = []
      let offset = 0
      for (;;) {
        const page = await getScanResults(sessionId, offset, PAGE)
        all.push(...page.files)
        offset += page.files.length
        if (page.files.length === 0 || offset >= page.total) break
      }
      discoveredIds = new Set(all.map(f => f.id))
      discoveredFiles.value = all
    } catch (e) {
      console.error('拉取完整扫描结果失败:', e)
    }
  }

  const currentSession = computed(() => {
    if (!currentSessionId.value) return null
    return sessions.value.find(s => s.id === currentSessionId.value) ?? null
  })

  function handleScanEvent(event: ScanEvent) {
    if (event.session_id) {
      if (currentSessionId.value !== event.session_id) {
        currentSessionId.value = event.session_id
      }
      // 事件可能先于 invoke 返回到达（小目录秒级扫完）：懒创建会话，避免进度更新被丢弃
      if (!sessions.value.some(s => s.id === event.session_id)) {
        sessions.value.push({
          id: event.session_id,
          disk_index: 0,
          scan_mode: 'deep',
          start_time: new Date().toISOString(),
          end_time: null,
          total_sectors: 0,
          scanned_sectors: 0,
          found_files: 0,
          status: 'scanning',
          progress: 0,
          speed_mbps: 0,
        })
      }
    }

    const session = currentSession.value
    if (session) {
      session.progress = event.progress
      session.found_files = event.found_files ?? session.found_files
      if (event.speed_mbps !== undefined) {
        session.speed_mbps = event.speed_mbps
      }
      if (event.current_path !== undefined) {
        session.current_path = event.current_path
      }
      if (event.path_progress !== undefined) {
        session.path_progress = event.path_progress
      }
    }

    if (event.message) {
      scanLog.value.push(`[${new Date().toLocaleTimeString()}] ${event.message}`)
      if (scanLog.value.length > 500) {
        scanLog.value = scanLog.value.slice(-300)
      }
      // 捕获后端降级提示（无管理员权限 → MFT 卷直读被跳过，强制删除文件扫不到）
      if (event.message.includes('需要管理员权限')) {
        const note = event.message.replace(/^.*?(需要管理员权限)/s, '$1')
        if (!scanWarnings.value.some((w) => w === note)) {
          scanWarnings.value = [...scanWarnings.value, note]
        }
      }
    }

    // Handle result_files batch from backend（深度阶段批次已不再内联文件，只推计数）
    if (event.result_files && Array.isArray(event.result_files)) {
      addDiscovered(event.result_files)
    }

    switch (event.status) {
      case 'scanning':
        isScanning.value = true
        isPaused.value = false
        break
      case 'paused':
        isPaused.value = true
        break
      case 'completed':
      case 'cancelled':
      case 'error':
        isScanning.value = false
        isPaused.value = false
        if (session) {
          session.status = event.status as ScanStatus
          session.end_time = new Date().toISOString()
        }
        // 结果数超过已收到的内联预览时，分页拉取后端保存的完整结果（数万条）
        if ((event.status === 'completed' || event.status === 'cancelled') && event.session_id) {
          const reported = event.found_files ?? 0
          if (reported > discoveredFiles.value.length) {
            void fetchFullResults(event.session_id)
          }
        }
        break
    }
  }

  async function startPathScan(paths: string[], mode: 'quick' | 'deep' | 'raw') {
    const { startPathScan } = await import('@/api/tauri')
    scanLog.value = []
    discoveredIds = new Set()
    discoveredFiles.value = []
    scanWarnings.value = []
    const sessionId = await startPathScan(paths, mode)
    currentSessionId.value = sessionId

    // 扫描事件可能已先于 invoke 返回注册了会话（甚至已完成）：
    // 仅补充扫描参数，绝不重置进度与状态
    const existing = sessions.value.find(s => s.id === sessionId)
    if (existing) {
      existing.target_paths = [...paths]
      existing.scan_mode = mode
      isScanning.value = existing.status === 'scanning' || existing.status === 'paused'
      isPaused.value = existing.status === 'paused'
      return sessionId
    }

    isScanning.value = true
    isPaused.value = false

    sessions.value.push({
      id: sessionId,
      disk_index: 0,
      scan_mode: mode,
      start_time: new Date().toISOString(),
      end_time: null,
      total_sectors: 0,
      scanned_sectors: 0,
      found_files: 0,
      status: 'scanning',
      progress: 0,
      speed_mbps: 0,
      target_paths: [...paths],
      current_path: paths[0],
      path_progress: 0,
    })

    return sessionId
  }

  async function startScan(diskIndex: number, mode: 'quick' | 'deep' | 'raw') {
    const { startScan } = await import('@/api/tauri')
    scanLog.value = []
    discoveredIds = new Set()
    discoveredFiles.value = []
    scanWarnings.value = []
    const sessionId = await startScan(diskIndex, mode)
    currentSessionId.value = sessionId

    // 扫描事件可能先于 invoke 返回到达（懒创建会话已存在）：
    // 仅补充扫描参数，绝不重置进度与状态，避免重复会话覆盖已完成状态
    const existing = sessions.value.find(s => s.id === sessionId)
    if (existing) {
      existing.disk_index = diskIndex
      existing.scan_mode = mode
      isScanning.value = existing.status === 'scanning' || existing.status === 'paused'
      isPaused.value = existing.status === 'paused'
      return sessionId
    }

    isScanning.value = true
    isPaused.value = false

    sessions.value.push({
      id: sessionId,
      disk_index: diskIndex,
      scan_mode: mode,
      start_time: new Date().toISOString(),
      end_time: null,
      total_sectors: 0,
      scanned_sectors: 0,
      found_files: 0,
      status: 'scanning',
      progress: 0,
      speed_mbps: 0,
    })

    return sessionId
  }

  async function pauseScan() {
    if (!currentSessionId.value) return
    const { pauseScan } = await import('@/api/tauri')
    await pauseScan(currentSessionId.value)
  }

  async function resumeScan() {
    if (!currentSessionId.value) return
    const { resumeScan } = await import('@/api/tauri')
    await resumeScan(currentSessionId.value)
  }

  async function cancelScan() {
    if (!currentSessionId.value) return
    const { cancelScan } = await import('@/api/tauri')
    await cancelScan(currentSessionId.value)
    isScanning.value = false
    isPaused.value = false
  }

  return {
    sessions,
    currentSessionId,
    currentSession,
    isScanning,
    isPaused,
    scanLog,
    scanWarnings,
    discoveredFiles,
    handleScanEvent,
    startPathScan,
    startScan,
    pauseScan,
    resumeScan,
    cancelScan,
  }
})
