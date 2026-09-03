use serde::{Deserialize, Serialize};

/// 磁盘信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub index: u32,
    pub model: String,
    pub serial: String,
    pub capacity_bytes: u64,
    pub free_bytes: u64,
    pub sector_size: u64,
    pub total_sectors: u64,
    pub partitions: Vec<PartitionInfo>,
    pub smart_status: Option<SmartStatus>,
    pub is_removable: bool,
    pub device_path: String,
}

/// 分区信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    pub start_sector: u64,
    pub size_sectors: u64,
    pub filesystem: String,
    pub mount_point: Option<String>,
    pub status: PartitionStatus,
    pub is_bootable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PartitionStatus {
    Active,
    Deleted,
    Hidden,
    Unknown,
}

/// 扫描会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSession {
    pub id: String,
    pub disk_index: u32,
    pub scan_mode: ScanMode,
    pub start_time: String,
    pub end_time: Option<String>,
    pub total_sectors: u64,
    pub scanned_sectors: u64,
    pub found_files: u64,
    pub status: ScanStatus,
    pub progress: f32,
    pub speed_mbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    Quick,
    Deep,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScanStatus {
    Pending,
    Scanning,
    Paused,
    Completed,
    Cancelled,
    Error,
}

/// 恢复文件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredFile {
    pub id: String,
    pub session_id: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: u64,
    pub original_path: String,
    pub recovered_path: Option<String>,
    pub signature_type: String,
    pub start_sector: u64,
    pub sha256: String,
    pub status: FileStatus,
    pub preview_available: bool,
    pub created_time: Option<String>,
    pub modified_time: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Found,
    Recoverable,
    Recovered,
    Corrupted,
}

/// S.M.A.R.T. 状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartStatus {
    pub healthy: bool,
    pub temperature: Option<u32>,
    pub power_on_hours: Option<u64>,
    pub attributes: Vec<SmartAttribute>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartAttribute {
    pub id: u8,
    pub name: String,
    pub current: u8,
    pub worst: u8,
    pub threshold: u8,
    pub raw_value: u64,
    pub status: SmartAttrStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SmartAttrStatus {
    Ok,
    Warning,
    Critical,
}

/// 找到的分区
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoundPartition {
    pub start_sector: u64,
    pub size_sectors: u64,
    pub filesystem: String,
    pub label: Option<String>,
    pub confidence: f32,
    pub is_bootable: bool,
}

/// 磁盘镜像进度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageProgress {
    pub total_sectors: u64,
    pub copied_sectors: u64,
    pub bad_sectors: u64,
    pub speed_mbps: f64,
    pub status: ImageStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageStatus {
    Creating,
    Completed,
    Error,
}
