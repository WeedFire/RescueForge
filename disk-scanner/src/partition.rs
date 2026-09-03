use anyhow::Result;

/// MBR 分区表条目（16 字节）
#[derive(Debug, Clone)]
pub struct MbrPartitionEntry {
    pub bootable: bool,
    pub start_head: u8,
    pub start_sector: u8,
    pub start_cylinder: u8,
    pub partition_type: u8,
    pub end_head: u8,
    pub end_sector: u8,
    pub end_cylinder: u8,
    pub first_sector: u32,
    pub total_sectors: u32,
}

/// GPT 分区表头
#[derive(Debug, Clone)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub partition_entry_size: u32,
    pub partition_array_crc32: u32,
}

/// GPT 分区条目（128 字节）
#[derive(Debug, Clone)]
pub struct GptPartitionEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub name: String,
}

/// 解析 MBR 分区表
pub fn parse_mbr(buffer: &[u8]) -> Result<Vec<MbrPartitionEntry>> {
    if buffer.len() < 512 {
        return Err(anyhow::anyhow!("Buffer too small for MBR"));
    }

    // MBR 签名检查
    if buffer[510] != 0x55 || buffer[511] != 0xAA {
        return Err(anyhow::anyhow!("Invalid MBR signature"));
    }

    let mut partitions = Vec::new();
    let partition_table_offset = 446; // 0x1BE

    for i in 0..4 {
        let offset = partition_table_offset + i * 16;
        let entry = &buffer[offset..offset + 16];

        let bootable = entry[0] == 0x80;
        let partition_type = entry[4];

        // Skip empty entries
        if partition_type == 0x00 {
            continue;
        }

        let first_sector = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
        let total_sectors = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]);

        partitions.push(MbrPartitionEntry {
            bootable,
            start_head: entry[1],
            start_sector: entry[2],
            start_cylinder: entry[3],
            partition_type,
            end_head: entry[5],
            end_sector: entry[6],
            end_cylinder: entry[7],
            first_sector,
            total_sectors,
        });
    }

    Ok(partitions)
}

/// 解析 GPT 分区表头
pub fn parse_gpt_header(buffer: &[u8]) -> Result<GptHeader> {
    if buffer.len() < 92 {
        return Err(anyhow::anyhow!("Buffer too small for GPT header"));
    }

    let mut signature = [0u8; 8];
    signature.copy_from_slice(&buffer[0..8]);

    if &signature != b"EFI PART" {
        return Err(anyhow::anyhow!("Invalid GPT signature"));
    }

    Ok(GptHeader {
        signature,
        revision: u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
        header_size: u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
        header_crc32: u32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]),
        current_lba: u64::from_le_bytes(buffer[24..32].try_into().unwrap()),
        backup_lba: u64::from_le_bytes(buffer[32..40].try_into().unwrap()),
        first_usable_lba: u64::from_le_bytes(buffer[40..48].try_into().unwrap()),
        last_usable_lba: u64::from_le_bytes(buffer[48..56].try_into().unwrap()),
        disk_guid: buffer[56..72].try_into().unwrap(),
        partition_entry_lba: u64::from_le_bytes(buffer[72..80].try_into().unwrap()),
        num_partition_entries: u32::from_le_bytes([buffer[80], buffer[81], buffer[82], buffer[83]]),
        partition_entry_size: u32::from_le_bytes([buffer[84], buffer[85], buffer[86], buffer[87]]),
        partition_array_crc32: u32::from_le_bytes([buffer[88], buffer[89], buffer[90], buffer[91]]),
    })
}

/// 判断分区文件系统类型
pub fn detect_filesystem(partition_type: u8) -> &'static str {
    match partition_type {
        0x01 => "FAT12",
        0x04 => "FAT16",
        0x05 => "Extended",
        0x06 => "FAT16B",
        0x07 => "NTFS/exFAT",
        0x0B => "FAT32",
        0x0C => "FAT32 (LBA)",
        0x0E => "FAT16 (LBA)",
        0x0F => "Extended (LBA)",
        0x82 => "Linux Swap",
        0x83 => "Linux (ext2/3/4)",
        0x85 => "Linux Extended",
        0x8E => "Linux LVM",
        0xA5 => "FreeBSD",
        0xA6 => "OpenBSD",
        0xA8 => "macOS UFS",
        0xA9 => "NetBSD",
        0xAB => "macOS Boot",
        0xAF => "macOS HFS+",
        0xEE => "GPT Protective",
        0xEF => "EFI System",
        _ => "Unknown",
    }
}
