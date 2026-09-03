use anyhow::Result;

/// 创建磁盘镜像 (.img/.dd)
pub fn create_disk_image(
    _device_path: &str,
    _output_path: &str,
    _skip_bad_sectors: bool,
) -> Result<()> {
    // TODO: 扇区级复制，支持坏道跳过
    Err(anyhow::anyhow!("Not implemented"))
}
