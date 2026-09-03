use serde::{Deserialize, Serialize};

/// Tauri 事件：扫描进度
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanEvent {
    pub status: String,
    pub progress: f32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Sidecar 通信消息（JSON Lines 协议）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SidecarMessage {
    #[serde(rename = "progress")]
    Progress {
        scanned_sectors: u64,
        total_sectors: u64,
        found_files: u64,
        speed_mbps: f64,
    },
    #[serde(rename = "file_found")]
    FileFound {
        filename: String,
        extension: String,
        size_bytes: u64,
        start_sector: u64,
        signature_type: String,
    },
    #[serde(rename = "log")]
    Log {
        level: String,
        message: String,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
    },
    #[serde(rename = "complete")]
    Complete {
        total_files: u64,
        duration_secs: f64,
    },
}

/// Sidecar 命令（主进程 -> Sidecar）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command")]
pub enum SidecarCommand {
    #[serde(rename = "scan")]
    Scan {
        device_path: String,
        scan_mode: String,
        signature_ids: Option<Vec<String>>,
    },
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "resume")]
    Resume,
    #[serde(rename = "cancel")]
    Cancel,
}
