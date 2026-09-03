// 磁盘信息
export interface DiskInfo {
  index: number
  model: string
  serial: string
  capacity_bytes: number
  free_bytes: number
  sector_size: number
  total_sectors: number
  partitions: PartitionInfo[]
  smart_status: SmartStatus | null
  is_removable: boolean
  device_path: string
}

// 分区信息
export interface PartitionInfo {
  start_sector: number
  size_sectors: number
  filesystem: string
  mount_point: string | null
  status: PartitionStatus
  is_bootable: boolean
}

export type PartitionStatus = 'active' | 'deleted' | 'hidden' | 'unknown'

// 扫描会话
export interface ScanSession {
  id: string
  disk_index: number
  scan_mode: ScanMode
  start_time: string
  end_time: string | null
  total_sectors: number
  scanned_sectors: number
  found_files: number
  status: ScanStatus
  progress: number
  speed_mbps: number
  target_paths?: string[]
  current_path?: string
  path_progress?: number
}

export type ScanMode = 'quick' | 'deep' | 'raw'
export type ScanStatus = 'pending' | 'scanning' | 'paused' | 'completed' | 'cancelled' | 'error'

// 恢复文件
export interface RecoveredFile {
  id: string
  session_id: string
  filename: string
  extension: string
  size_bytes: number
  original_path: string
  recovered_path: string | null
  signature_type: string
  start_sector: number
  sha256: string
  status: FileStatus
  preview_available: boolean
  created_time: string | null
  modified_time: string | null
}

export type FileStatus = 'found' | 'recoverable' | 'recovered' | 'corrupted'

// 文件签名
export interface FileSignature {
  extension: string
  description: string
  header_bytes: number[]
  footer_bytes: number[] | null
  max_size_bytes: number | null
}

// 扫描事件
export interface ScanEvent {
  status: string
  progress: number
  message: string
  found_files?: number
  speed_mbps?: number
  session_id?: string
  current_path?: string
  path_progress?: number
  result_files?: FoundFileInfo[]
}

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
}

// S.M.A.R.T.
export interface SmartStatus {
  healthy: boolean
  temperature: number
  power_on_hours: number
  attributes: SmartAttribute[]
  warning: string | null
}

export interface SmartAttribute {
  id: number
  name: string
  current: number
  worst: number
  threshold: number
  raw_value: number
  status: 'ok' | 'warning' | 'critical'
}

// 分区修复
export interface FoundPartition {
  start_sector: number
  size_sectors: number
  filesystem: string
  label: string | null
  confidence: number
  is_bootable: boolean
}

// 磁盘镜像
export interface ImageProgress {
  total_sectors: number
  copied_sectors: number
  bad_sectors: number
  speed_mbps: number
  status: 'creating' | 'completed' | 'error'
}

// UI 主题：eye=护眼色（默认）
export type ThemeMode = 'eye' | 'dark' | 'light'

// 用户设置
export interface UserSettings {
  theme: ThemeMode
  language: string
  default_recovery_path: string
  auto_update: boolean
  show_hidden_files: boolean
}
