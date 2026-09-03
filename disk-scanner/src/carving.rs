use anyhow::Result;
use rescueforge_core::types::RecoveredFile;
use rescueforge_core::signatures::FileSignature;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;

/// RAW 文件雕刻引擎
pub struct CarvingEngine {
    signatures: Vec<FileSignature>,
    chunk_size: usize,
    scanned_sectors: Arc<AtomicU64>,
    found_files: Arc<AtomicU64>,
    cancelled: Arc<AtomicBool>,
}

impl CarvingEngine {
    pub fn new(signatures: Vec<FileSignature>) -> Self {
        Self {
            signatures: signatures.into_iter().filter(|s| s.enabled).collect(),
            chunk_size: 64 * 1024 * 1024, // 64 MB chunks
            scanned_sectors: Arc::new(AtomicU64::new(0)),
            found_files: Arc::new(AtomicU64::new(0)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 执行文件雕刻
    pub fn carve(&mut self, _data: &[u8], _sector_offset: u64) -> Result<Vec<RecoveredFile>> {
        // TODO: 实现多线程 RAW 雕刻逻辑
        Ok(vec![])
    }

    /// 取消雕刻
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// 获取进度
    pub fn get_progress(&self) -> (u64, u64) {
        (
            self.scanned_sectors.load(Ordering::Relaxed),
            self.found_files.load(Ordering::Relaxed),
        )
    }
}

/// 碎片文件拼接
pub fn reassemble_fragments(fragments: &[(Vec<u8>, u64)]) -> Result<Vec<u8>> {
    // 按扇区偏移排序
    let mut sorted: Vec<_> = fragments.iter().collect();
    sorted.sort_by_key(|(_, offset)| *offset);

    let total_size: usize = sorted.iter().map(|(data, _)| data.len()).sum();
    let mut result = Vec::with_capacity(total_size);

    for (data, _) in sorted {
        result.extend_from_slice(data);
    }

    Ok(result)
}
