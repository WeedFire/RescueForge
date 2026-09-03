use anyhow::Result;
use rescueforge_core::types::RecoveredFile;

/// NTFS 文件系统解析
pub fn parse_ntfs(_buffer: &[u8], _sector_offset: u64) -> Result<Vec<RecoveredFile>> {
    // TODO: 实现 NTFS MFT 解析
    Ok(vec![])
}

/// 检查 NTFS 卷
pub fn is_ntfs_volume(buffer: &[u8]) -> bool {
    if buffer.len() < 8 { return false; }
    &buffer[3..8] == b"NTFS "
}
