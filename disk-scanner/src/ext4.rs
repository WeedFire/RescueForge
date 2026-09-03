use anyhow::Result;
use rescueforge_core::types::RecoveredFile;

/// ext4 文件系统解析
pub fn parse_ext4(_buffer: &[u8], _sector_offset: u64) -> Result<Vec<RecoveredFile>> {
    // TODO: 实现 ext4 inode 遍历
    Ok(vec![])
}

/// 检查 ext 卷
pub fn is_ext_volume(buffer: &[u8]) -> bool {
    if buffer.len() < 0x438 + 2 { return false; }
    // ext2/3/4 magic at offset 0x438
    buffer[0x438] == 0x53 && buffer[0x439] == 0xEF
}
