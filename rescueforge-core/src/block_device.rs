use anyhow::Result;

/// 跨平台块设备读取 trait
///
/// 定义原始磁盘扇区读取的统一接口，各平台分别实现：
/// - Windows: CreateFileW + FILE_FLAG_NO_BUFFERING
/// - Linux: /dev/sdX
/// - macOS: /dev/rdiskX
pub trait BlockDevice {
    /// 读取从 start_sector 开始的 count 个扇区到 buffer
    fn read_sectors(&mut self, start_sector: u64, count: u64, buffer: &mut [u8]) -> Result<()>;

    /// 获取磁盘几何信息
    fn get_geometry(&self) -> Result<DiskGeometry>;
}

/// 磁盘几何信息
#[derive(Debug, Clone)]
pub struct DiskGeometry {
    pub sector_size: u64,
    pub total_sectors: u64,
    pub capacity_bytes: u64,
    pub cylinders: u32,
    pub tracks_per_cylinder: u32,
    pub sectors_per_track: u32,
}
