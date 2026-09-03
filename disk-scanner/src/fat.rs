use anyhow::Result;
use rescueforge_core::types::RecoveredFile;

/// FAT32/exFAT 文件系统解析
pub fn parse_fat(_buffer: &[u8], _sector_offset: u64) -> Result<Vec<RecoveredFile>> {
    // TODO: 实现 FAT 目录项解析
    Ok(vec![])
}

/// 检查 FAT 卷
pub fn is_fat_volume(buffer: &[u8]) -> bool {
    if buffer.len() < 512 { return false; }
    // FAT BS jmp instruction + OEM name
    buffer[0] == 0xEB || buffer[0] == 0xE9
}

/// 检查 exFAT 卷
pub fn is_exfat_volume(buffer: &[u8]) -> bool {
    if buffer.len() < 11 { return false; }
    &buffer[3..11] == b"EXFAT   "
}
