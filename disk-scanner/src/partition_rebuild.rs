use anyhow::Result;
use rescueforge_core::types::FoundPartition;

/// 搜索丢失的分区
pub fn search_lost_partitions(_buffer: &[u8], _sector_count: u64) -> Result<Vec<FoundPartition>> {
    // TODO: 扫描扇区，查找分区引导扇区特征
    Ok(vec![])
}

/// 重建 MBR/GPT 分区表
pub fn rebuild_partition_table(
    _device_path: &str,
    _partitions: &[FoundPartition],
) -> Result<()> {
    // TODO: 写入新的分区表（需用户确认）
    Err(anyhow::anyhow!("Not implemented"))
}
