import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { DiskInfo, ScanEvent, SmartStatus, FoundPartition, ImageProgress } from '@/types'

// ===== 磁盘操作 =====
export async function getDisks(): Promise<DiskInfo[]> {
  return invoke('get_disks')
}

export async function getSmartStatus(diskIndex: number): Promise<SmartStatus> {
  return invoke('get_smart_status', { diskIndex })
}

// ===== 文件夹选择 =====
export async function selectFolder(): Promise<string | null> {
  return invoke('select_folder')
}

// ===== 路径校验 =====
export interface PathValidation {
  normalized: string
  kind: 'dir' | 'file' | 'missing'
}

export async function validatePath(path: string): Promise<PathValidation> {
  return invoke('validate_path', { path })
}

// ===== 扫描操作 =====
export async function startPathScan(
  paths: string[],
  mode: 'quick' | 'deep' | 'raw',
  signatureIds?: string[]
): Promise<string> {
  return invoke('start_path_scan', { paths, mode, signatureIds })
}

export async function startScan(
  diskIndex: number,
  mode: 'quick' | 'deep' | 'raw',
  signatureIds?: string[]
): Promise<string> {
  return invoke('start_scan', { diskIndex, mode, signatureIds })
}

export async function pauseScan(sessionId: string): Promise<void> {
  return invoke('pause_scan', { sessionId })
}

export async function resumeScan(sessionId: string): Promise<void> {
  return invoke('resume_scan', { sessionId })
}

export async function cancelScan(sessionId: string): Promise<void> {
  return invoke('cancel_scan', { sessionId })
}

// ===== 扫描结果分页拉取 =====
// 深度扫描可能发现数万文件，事件只内联前 2000 条预览；
// 完整结果由后端保存，通过此命令分页获取，避免巨型消息卡死前端。
export interface ScanResultsPage {
  total: number
  offset: number
  files: import('@/types').FoundFileInfo[]
}

export async function getScanResults(
  sessionId: string,
  offset = 0,
  limit = 1000
): Promise<ScanResultsPage> {
  return invoke('get_scan_results', { sessionId, offset, limit })
}

// ===== 扫描事件监听 =====
export function onScanEvent(callback: (event: ScanEvent) => void): Promise<UnlistenFn> {
  return listen<ScanEvent>('scan-event', (event) => {
    callback(event.payload)
  })
}

// ===== 后端运行日志（排障） =====
export async function getAppLog(tailLines = 200): Promise<{ path: string; lines: string[] }> {
  return invoke('get_app_log', { tailLines })
}

export async function writeAppLog(level: string, message: string): Promise<void> {
  return invoke('write_app_log', { level, message })
}

// ===== 自动冒烟测试开关（环境变量 RF_AUTOTEST=1） =====
export async function getAutotestFlag(): Promise<boolean> {
  return invoke('get_autotest_flag')
}

// ===== 文件恢复 =====
export async function recoverFiles(
  fileIds: string[],
  outputPath: string
): Promise<{ success: number; failed: number }> {
  return invoke('recover_files', { fileIds, outputPath })
}

export async function previewFile(filepath: string): Promise<Uint8Array> {
  return invoke('preview_file', { filepath })
}

// ===== 在文件管理器中打开目录 =====
export async function revealInExplorer(path: string): Promise<void> {
  return invoke('reveal_in_explorer', { path })
}

// ===== 分区修复 =====
export async function searchLostPartitions(diskIndex: number): Promise<FoundPartition[]> {
  return invoke('search_lost_partitions', { diskIndex })
}

export async function rebuildPartitionTable(
  diskIndex: number,
  partitions: FoundPartition[]
): Promise<string> {
  return invoke('rebuild_partition_table', { diskIndex, partitions })
}

export async function backupBootSector(diskIndex: number, partitionIndex: number): Promise<string> {
  return invoke('backup_boot_sector', { diskIndex, partitionIndex })
}

export async function restoreBootSector(
  diskIndex: number,
  partitionIndex: number,
  backupPath: string
): Promise<string> {
  return invoke('restore_boot_sector', { diskIndex, partitionIndex, backupPath })
}

// ===== 磁盘镜像 =====
export async function createDiskImage(
  diskIndex: number,
  outputPath: string,
  skipBadSectors: boolean
): Promise<string> {
  return invoke('create_disk_image', { diskIndex, outputPath, skipBadSectors })
}

export async function cancelDiskImage(): Promise<void> {
  return invoke('cancel_disk_image')
}

export function onImageProgress(callback: (progress: ImageProgress) => void): Promise<UnlistenFn> {
  return listen<ImageProgress>('image-progress', (event) => {
    callback(event.payload)
  })
}

// ===== 权限（管理员提权） =====
export interface PrivilegeStatus {
  elevated: boolean
}

export async function getPrivilegeStatus(): Promise<PrivilegeStatus> {
  return invoke('get_privilege_status')
}

export async function restartElevated(): Promise<void> {
  return invoke('restart_elevated')
}

// ===== 设置 =====
export async function getSettings(): Promise<Record<string, unknown>> {
  return invoke('get_settings')
}

export async function updateSettings(settings: Record<string, unknown>): Promise<void> {
  return invoke('update_settings', { settings })
}
