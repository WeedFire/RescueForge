//! 分区修复：真实实现
//! - 搜索丢失分区：读取物理磁盘的 MBR / GPT 分区表 + 引导扇区签名扫描（NTFS / FAT32 / FAT16 / exFAT）
//! - 重建分区表：写入 MBR 分区表（写入前自动备份原 MBR）
//! - 备份 / 恢复引导扇区：对卷设备 `\\.\X:` 读写前 512 字节
//! - 磁盘镜像：按 4MB 块读取卷数据写入镜像文件，实时推送 image-progress 事件
//!
//! 设备访问（物理磁盘/卷设备）需要管理员权限，未提权时返回明确的错误提示。

use rescueforge_core::types::{FoundPartition, ImageProgress, ImageStatus};
use std::sync::atomic::{AtomicBool, Ordering};

// ===== IOCTL / FSCTL 常量 =====
const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
const FSCTL_UNLOCK_VOLUME: u32 = 0x0009_001C;
const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
const IOCTL_DISK_UPDATE_PROPERTIES: u32 = 0x0007_0140;

/// 丢失分区签名扫描的步进（1MB 对齐，覆盖现代分区对齐方式）
const SCAN_STEP_SECTORS: u64 = 2048;
/// 签名扫描的最大范围（磁盘头部 2GB，兼顾速度与常见场景）
const SCAN_LIMIT_SECTORS: u64 = 4 * 1024 * 1024; // 2GB / 512B

/// 镜像任务取消标志（全局单一任务）
static IMAGE_CANCEL: AtomicBool = AtomicBool::new(false);

// ===== 纯解析层（可测试） =====

#[derive(Debug, Clone, PartialEq)]
pub struct MbrPartition {
    pub bootable: bool,
    pub ptype: u8,
    pub start_lba: u32,
    pub size_lba: u32,
}

/// 解析 MBR 扇区的 4 个分区表项（要求 55AA 签名）
pub fn parse_mbr_entries(sector: &[u8]) -> Vec<MbrPartition> {
    let mut out = Vec::new();
    if sector.len() < 512 || sector[510] != 0x55 || sector[511] != 0xAA {
        return out;
    }
    for i in 0..4 {
        let off = 446 + i * 16;
        let ptype = sector[off + 4];
        if ptype == 0 { continue; }
        out.push(MbrPartition {
            bootable: sector[off] == 0x80,
            ptype,
            start_lba: u32::from_le_bytes(sector[off + 8..off + 12].try_into().unwrap()),
            size_lba: u32::from_le_bytes(sector[off + 12..off + 16].try_into().unwrap()),
        });
    }
    out
}

/// 在保留原引导代码（0..446）的前提下，把分区表项写回 MBR 扇区
pub fn build_mbr_sector(original: &[u8], entries: &[MbrPartition]) -> Option<[u8; 512]> {
    if original.len() < 512 || entries.len() > 4 { return None; }
    let mut buf = [0u8; 512];
    buf.copy_from_slice(&original[..512]);
    for i in 0..4 {
        let off = 446 + i * 16;
        buf[off..off + 16].copy_from_slice(&[0u8; 16]);
        if let Some(e) = entries.get(i) {
            buf[off] = if e.bootable { 0x80 } else { 0x00 };
            buf[off + 4] = e.ptype;
            buf[off + 8..off + 12].copy_from_slice(&e.start_lba.to_le_bytes());
            buf[off + 12..off + 16].copy_from_slice(&e.size_lba.to_le_bytes());
        }
    }
    buf[510] = 0x55;
    buf[511] = 0xAA;
    Some(buf)
}

#[derive(Debug, Clone)]
pub struct GptHeader {
    pub partition_entry_lba: u64,
    pub num_entries: u32,
    pub entry_size: u32,
}

/// 解析 GPT 头（LBA1 扇区）
pub fn parse_gpt_header(sector: &[u8]) -> Option<GptHeader> {
    if sector.len() < 0x58 || &sector[0..8] != b"EFI PART" { return None; }
    Some(GptHeader {
        partition_entry_lba: u64::from_le_bytes(sector[0x48..0x50].try_into().unwrap()),
        num_entries: u32::from_le_bytes(sector[0x50..0x54].try_into().unwrap()),
        entry_size: u32::from_le_bytes(sector[0x54..0x58].try_into().unwrap()),
    })
}

#[derive(Debug, Clone)]
pub struct GptPartition {
    pub start_lba: u64,
    pub end_lba: u64,
    pub name: String,
}

/// 解析 GPT 分区表项数组
pub fn parse_gpt_entries(buf: &[u8], entry_size: usize) -> Vec<GptPartition> {
    let mut out = Vec::new();
    if entry_size < 128 { return out; }
    let mut off = 0usize;
    while off + entry_size <= buf.len() {
        let e = &buf[off..off + entry_size];
        // type GUID 全零 = 空表项
        if e[0..16].iter().all(|&b| b == 0) { off += entry_size; continue; }
        let start_lba = u64::from_le_bytes(e[32..40].try_into().unwrap());
        let end_lba = u64::from_le_bytes(e[40..48].try_into().unwrap());
        let name_u16: Vec<u16> = e[56..128].chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = name_u16.iter().position(|&c| c == 0).unwrap_or(name_u16.len());
        out.push(GptPartition {
            start_lba,
            end_lba,
            name: String::from_utf16_lossy(&name_u16[..end]),
        });
        off += entry_size;
    }
    out
}

/// 识别引导扇区的文件系统签名
pub fn detect_boot_signature(sector: &[u8]) -> Option<&'static str> {
    if sector.len() < 512 || sector[510] != 0x55 || sector[511] != 0xAA { return None; }
    if &sector[3..11] == b"NTFS    " { return Some("NTFS"); }
    if &sector[3..11] == b"EXFAT   " { return Some("exFAT"); }
    if &sector[82..90] == b"FAT32   " { return Some("FAT32"); }
    if &sector[54..59] == b"FAT16" || &sector[54..59] == b"FAT12" { return Some("FAT"); }
    None
}

fn u16le(b: &[u8], off: usize) -> u16 { u16::from_le_bytes(b[off..off + 2].try_into().unwrap()) }
fn u32le(b: &[u8], off: usize) -> u32 { u32::from_le_bytes(b[off..off + 4].try_into().unwrap()) }
fn u64le(b: &[u8], off: usize) -> u64 { u64::from_le_bytes(b[off..off + 8].try_into().unwrap()) }

/// 从引导扇区推断分区总扇区数（用于丢失分区的大小估计）
pub fn fs_size_from_boot(sector: &[u8], fs: &str) -> Option<u64> {
    if sector.len() < 512 { return None; }
    match fs {
        "NTFS" => { let n = u64le(sector, 0x28); if n > 0 { Some(n) } else { None } }
        "FAT32" => { let n = u32le(sector, 0x20) as u64; if n > 0 { Some(n) } else { None } }
        "FAT" => {
            let big = u32le(sector, 0x20) as u64;
            if big > 0 { Some(big) } else {
                let small = u16le(sector, 0x13) as u64;
                if small > 0 { Some(small) } else { None }
            }
        }
        "exFAT" => { let n = u64le(sector, 0x48); if n > 0 { Some(n) } else { None } }
        _ => None,
    }
}

/// FAT32/FAT16 卷标（引导扇区内），无有效卷标返回 None
pub fn fat_label_from_boot(sector: &[u8], fs: &str) -> Option<String> {
    let range = match fs {
        "FAT32" => (71, 82),
        "FAT" => (43, 54),
        _ => return None,
    };
    if sector.len() < range.1 { return None; }
    let raw = &sector[range.0..range.1];
    // 卷标区可能包含 0x00 填充：只保留可打印字符（>= 0x20）
    let label: String = raw.iter()
        .take_while(|&&b| b >= 0x20)
        .map(|&b| b as char)
        .collect::<String>()
        .trim_end()
        .to_string();
    if label.is_empty() || label == "NO NAME" { None } else { Some(label) }
}

// ===== 设备层（Windows） =====

#[cfg(target_os = "windows")]
mod dev {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, GetFileSizeEx, ReadFile, SetFilePointerEx, WriteFile,
        FILE_BEGIN, FILE_FLAG_NO_BUFFERING, FILE_FLAG_RANDOM_ACCESS, FILE_FLAG_WRITE_THROUGH,
        FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    pub struct DevHandle(pub HANDLE);
    impl Drop for DevHandle {
        fn drop(&mut self) { unsafe { let _ = CloseHandle(self.0); } }
    }

    fn friendly_open_err(e: windows::core::Error, what: &str) -> String {
        let code = e.code().0 as u32;
        // ERROR_ACCESS_DENIED=5 ERROR_SHARING_VIOLATION=32
        if code == 5 {
            format!("{}: 需要管理员权限，请点击界面上的「以管理员身份重启」后重试", what)
        } else {
            format!("{}: {}", what, e)
        }
    }

    /// 打开物理磁盘 `\\.\PhysicalDrive{index}`
    pub fn open_physical_drive(index: u32, write: bool) -> Result<DevHandle, String> {
        let path = format!(r"\\.\PhysicalDrive{}", index);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let access = if write { GENERIC_READ.0 | GENERIC_WRITE.0 } else { GENERIC_READ.0 };
        let h = unsafe {
            CreateFileW(
                PCWSTR::from_raw(wide.as_ptr()), access,
                FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING,
                FILE_FLAG_RANDOM_ACCESS, None,
            )
        }.map_err(|e| friendly_open_err(e, &format!("无法打开物理磁盘 {}", index)))?;
        Ok(DevHandle(h))
    }

    /// 打开卷设备 `\\.\X:`
    pub fn open_volume(drive: char, write: bool) -> Result<DevHandle, String> {
        let path = format!(r"\\.\{}:", drive);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let access = if write { GENERIC_READ.0 | GENERIC_WRITE.0 } else { GENERIC_READ.0 };
        let h = unsafe {
            CreateFileW(
                PCWSTR::from_raw(wide.as_ptr()), access,
                FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING,
                FILE_FLAG_RANDOM_ACCESS, None,
            )
        }.map_err(|e| friendly_open_err(e, &format!("无法打开卷 {}", drive)))?;
        Ok(DevHandle(h))
    }

    pub fn read_at(h: HANDLE, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        unsafe {
            SetFilePointerEx(h, offset as i64, None, FILE_BEGIN)
                .map_err(|e| format!("定位失败(offset={}): {}", offset, e))?;
        }
        let mut got = 0usize;
        while got < len {
            let mut r = 0u32;
            let ok = unsafe { ReadFile(h, Some(&mut buf[got..]), Some(&mut r), None) };
            if ok.is_err() || r == 0 { break; }
            got += r as usize;
        }
        if got < len {
            return Err(format!("读取短读: 需要 {} 字节, 实际 {} 字节", len, got));
        }
        Ok(buf)
    }

    pub fn write_at(h: HANDLE, offset: u64, data: &[u8]) -> Result<(), String> {
        unsafe {
            SetFilePointerEx(h, offset as i64, None, FILE_BEGIN)
                .map_err(|e| format!("定位失败(offset={}): {}", offset, e))?;
            let mut written = 0u32;
            WriteFile(h, Some(data), Some(&mut written), None)
                .map_err(|e| format!("写入失败: {}", e))?;
            if written as usize != data.len() {
                return Err(format!("写入不完整: {} / {}", written, data.len()));
            }
            let _ = FlushFileBuffers(h);
        }
        Ok(())
    }

    pub fn lock_volume(h: HANDLE) -> Result<(), String> {
        let mut ret = 0u32;
        unsafe {
            DeviceIoControl(h, FSCTL_LOCK_VOLUME, None, 0, None, 0, Some(&mut ret), None)
                .map_err(|e| format!("锁定卷失败（请关闭正在使用该卷的程序）: {}", e))?;
        }
        Ok(())
    }

    pub fn unlock_volume(h: HANDLE) {
        let mut ret = 0u32;
        unsafe { let _ = DeviceIoControl(h, FSCTL_UNLOCK_VOLUME, None, 0, None, 0, Some(&mut ret), None); }
    }

    pub fn update_disk_properties(h: HANDLE) {
        let mut ret = 0u32;
        unsafe { let _ = DeviceIoControl(h, IOCTL_DISK_UPDATE_PROPERTIES, None, 0, None, 0, Some(&mut ret), None); }
    }

    /// 物理磁盘总字节数
    pub fn disk_length(h: HANDLE) -> Result<u64, String> {
        let mut len = 0u64;
        unsafe {
            DeviceIoControl(
                h, IOCTL_DISK_GET_LENGTH_INFO, None, 0,
                Some(&mut len as *mut u64 as *mut _), 8, None, None,
            ).map_err(|e| format!("获取磁盘容量失败: {}", e))?;
        }
        Ok(len)
    }

    /// 卷总字节数
    pub fn volume_length(h: HANDLE) -> Result<u64, String> {
        let mut len = 0i64;
        unsafe {
            if GetFileSizeEx(h, &mut len).is_ok() && len > 0 {
                return Ok(len as u64);
            }
        }
        disk_length(h)
    }

    /// 卷/磁盘设备是否可读（快速探测，用于镜像前的错误提示）
    pub fn probe_readable(h: HANDLE) -> Result<(), String> {
        match read_at(h, 0, 512) {
            Ok(_) => Ok(()),
            Err(_) => Err(format!("设备不可读 (last error: {:?})", unsafe { GetLastError() })),
        }
    }

    /// 打开卷时使用无缓冲（磁盘直读要求扇区对齐），返回独立句柄
    pub fn open_volume_no_buffering(drive: char) -> Result<DevHandle, String> {
        let path = format!(r"\\.\{}:", drive);
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let h = unsafe {
            CreateFileW(
                PCWSTR::from_raw(wide.as_ptr()), GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_RANDOM_ACCESS, None,
            )
        }.map_err(|e| friendly_open_err(e, &format!("无法打开卷 {}", drive)))?;
        Ok(DevHandle(h))
    }
}

#[cfg(target_os = "windows")]
use dev::*;

/// 备份目录：%USERPROFILE%\RescueForgeBackups
fn backup_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::PathBuf::from(home).join("RescueForgeBackups");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {}", e))?;
    Ok(dir)
}

fn timestamp_tag() -> String {
    // 简单的本地时间戳（秒级）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{}", secs)
}

// ===== Tauri 命令 =====

/// 搜索丢失的分区：读取分区表 + 引导扇区签名扫描
#[tauri::command]
pub async fn search_lost_partitions(disk_index: u32) -> Result<Vec<FoundPartition>, String> {
    crate::app_log!("INFO", "search_lost_partitions: 磁盘 {}", disk_index);
    #[cfg(not(target_os = "windows"))]
    { let _ = disk_index; return Err("分区修复目前仅支持 Windows".into()); }

    #[cfg(target_os = "windows")]
    {
        let dev = open_physical_drive(disk_index, false)?;
        let total_bytes = disk_length(dev.0)?;
        let total_sectors = total_bytes / 512;
        crate::app_log!("INFO", "磁盘 {} 容量 {} MB ({} 扇区)", disk_index, total_bytes >> 20, total_sectors);

        let mbr = read_at(dev.0, 0, 512)?;
        let mut found: Vec<FoundPartition> = Vec::new();
        let mut covered: Vec<(u64, u64)> = Vec::new();

        let mbr_entries = parse_mbr_entries(&mbr);
        let is_gpt = mbr_entries.iter().any(|e| e.ptype == 0xEE);

        if is_gpt {
            // GPT：读 LBA1 头 + 表项数组
            let hdr_sector = read_at(dev.0, 512, 512)?;
            if let Some(g) = parse_gpt_header(&hdr_sector) {
                let bytes = g.num_entries as usize * g.entry_size as usize;
                let buf = read_at(dev.0, g.partition_entry_lba * 512, bytes)?;
                for p in parse_gpt_entries(&buf, g.entry_size as usize) {
                    let size = p.end_lba.saturating_sub(p.start_lba) + 1;
                    let fs = read_at(dev.0, p.start_lba * 512, 512)
                        .ok().and_then(|s| detect_boot_signature(&s))
                        .unwrap_or("未知").to_string();
                    found.push(FoundPartition {
                        start_sector: p.start_lba,
                        size_sectors: size,
                        filesystem: fs,
                        label: if p.name.is_empty() { None } else { Some(p.name.clone()) },
                        confidence: 1.0,
                        is_bootable: false,
                    });
                    covered.push((p.start_lba, p.start_lba + size));
                }
            }
        } else {
            for e in &mbr_entries {
                if e.ptype == 0x05 || e.ptype == 0x0F { continue; } // 扩展分区，跳过容器
                let boot = read_at(dev.0, e.start_lba as u64 * 512, 512).ok();
                let fs = boot.as_ref().and_then(|b| detect_boot_signature(b)).unwrap_or("未知");
                let label = boot.as_ref().and_then(|b| fat_label_from_boot(b, fs));
                found.push(FoundPartition {
                    start_sector: e.start_lba as u64,
                    size_sectors: e.size_lba as u64,
                    filesystem: fs.to_string(),
                    label,
                    confidence: 1.0,
                    is_bootable: e.bootable,
                });
                covered.push((e.start_lba as u64, e.start_lba as u64 + e.size_lba as u64));
            }
        }
        crate::app_log!("INFO", "磁盘 {} 分区表内分区: {} 个 (GPT={})", disk_index, found.len(), is_gpt);

        // 签名扫描：在磁盘头部区间按 1MB 对齐寻找「表外」的引导扇区（丢失分区）
        let limit = SCAN_LIMIT_SECTORS.min(total_sectors);
        let mut lba = 0u64;
        let mut lost = 0u64;
        while lba < limit {
            if lba * 512 >= total_bytes { break; }
            let in_known = covered.iter().any(|(s, e)| lba >= *s && lba < *e);
            if !in_known {
                if let Ok(sec) = read_at(dev.0, lba * 512, 512) {
                    if let Some(fs) = detect_boot_signature(&sec) {
                        let size = fs_size_from_boot(&sec, fs).unwrap_or(0);
                        let label = fat_label_from_boot(&sec, fs);
                        found.push(FoundPartition {
                            start_sector: lba,
                            size_sectors: size,
                            filesystem: fs.to_string(),
                            label,
                            confidence: 0.75,
                            is_bootable: false,
                        });
                        covered.push((lba, lba + size.max(1)));
                        lost += 1;
                        crate::app_log!("INFO", "发现丢失分区候选: LBA {} fs={} size={} 扇区", lba, fs, size);
                    }
                }
            }
            lba += SCAN_STEP_SECTORS;
        }
        crate::app_log!("INFO", "签名扫描结束: 新发现 {} 个丢失分区候选", lost);
        Ok(found)
    }
}

/// 重建分区表（目前支持 MBR 磁盘；写入前自动备份原 MBR）
#[tauri::command]
pub async fn rebuild_partition_table(
    disk_index: u32,
    partitions: Vec<FoundPartition>,
) -> Result<String, String> {
    crate::app_log!("INFO", "rebuild_partition_table: 磁盘 {} 分区数 {}", disk_index, partitions.len());
    if partitions.is_empty() { return Err("没有可写入的分区".into()); }
    if partitions.len() > 4 { return Err("MBR 分区表最多支持 4 个主分区".into()); }
    if !crate::privilege::is_elevated() {
        return Err("重建分区表需要管理员权限，请先「以管理员身份重启」".into());
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = disk_index; return Err("分区修复目前仅支持 Windows".into()); }

    #[cfg(target_os = "windows")]
    {
        let dev = open_physical_drive(disk_index, true)?;
        dev::lock_volume(dev.0)?;
        let original = read_at(dev.0, 0, 512)?;
        if original[510] != 0x55 || original[511] != 0xAA {
            return Err("目标磁盘没有有效的 MBR 签名（可能是 GPT 磁盘），出于安全已拒绝写入".into());
        }
        if original[446..450].iter().any(|&b| b != 0) && parse_mbr_entries(&original).iter().any(|e| e.ptype == 0xEE) {
            return Err("目标磁盘为 GPT 分区表，暂不支持重建，已拒绝写入".into());
        }

        // 写入前备份原 MBR
        let backup_path = backup_dir()?.join(format!("mbr_disk{}_{}.bin", disk_index, timestamp_tag()));
        std::fs::write(&backup_path, &original)
            .map_err(|e| format!("备份原分区表失败: {}", e))?;
        crate::app_log!("INFO", "原 MBR 已备份到 {}", backup_path.display());

        let entries: Vec<MbrPartition> = partitions.iter().map(|p| MbrPartition {
            bootable: p.is_bootable,
            ptype: match p.filesystem.as_str() {
                "NTFS" => 0x07,
                "FAT32" => 0x0C,
                "FAT" => 0x06,
                "exFAT" => 0x07,
                _ => 0x07,
            },
            start_lba: p.start_sector as u32,
            size_lba: p.size_sectors.min(u32::MAX as u64) as u32,
        }).collect();

        let new_mbr = build_mbr_sector(&original, &entries)
            .ok_or("构建新分区表失败")?;
        dev::write_at(dev.0, 0, &new_mbr)?;
        dev::update_disk_properties(dev.0);
        dev::unlock_volume(dev.0);
        crate::app_log!("INFO", "磁盘 {} 分区表已重建: {} 个分区", disk_index, entries.len());
        Ok(format!("分区表重建成功：写入 {} 个分区。原分区表已备份到 {}", partitions.len(), backup_path.display()))
    }
}

/// 备份卷引导扇区（前 512 字节）
#[tauri::command]
pub async fn backup_boot_sector(
    disk_index: u32,
    partition_index: u32,
) -> Result<String, String> {
    crate::app_log!("INFO", "backup_boot_sector: 磁盘 {} 分区 {}", disk_index, partition_index);
    let _ = partition_index; // 当前每盘一个卷模型，分区索引保留兼容
    #[cfg(not(target_os = "windows"))]
    { let _ = disk_index; return Err("分区修复目前仅支持 Windows".into()); }

    #[cfg(target_os = "windows")]
    {
        let drive = crate::commands::disk::drive_letter_for_index(disk_index)
            .ok_or_else(|| format!("找不到磁盘 {}", disk_index))?;
        let dev = open_volume(drive, false)?;
        let sector = read_at(dev.0, 0, 512)?;
        let fs = detect_boot_signature(&sector).unwrap_or("未知");
        let out = backup_dir()?.join(format!("boot_{}_{}.bin", drive, timestamp_tag()));
        std::fs::write(&out, &sector).map_err(|e| format!("写入备份文件失败: {}", e))?;
        crate::app_log!("INFO", "卷 {} 引导扇区({})已备份到 {}", drive, fs, out.display());
        Ok(format!("引导扇区({})已备份: {}", fs, out.display()))
    }
}

/// 从备份文件恢复卷引导扇区
#[tauri::command]
pub async fn restore_boot_sector(
    disk_index: u32,
    partition_index: u32,
    backup_path: String,
) -> Result<String, String> {
    crate::app_log!("INFO", "restore_boot_sector: 磁盘 {} <- {}", disk_index, backup_path);
    let _ = partition_index;
    if !crate::privilege::is_elevated() {
        return Err("恢复引导扇区需要管理员权限，请先「以管理员身份重启」".into());
    }
    let data = std::fs::read(&backup_path).map_err(|e| format!("读取备份文件失败: {}", e))?;
    if data.len() != 512 {
        return Err(format!("备份文件大小必须为 512 字节，实际 {}", data.len()));
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = disk_index; return Err("分区修复目前仅支持 Windows".into()); }

    #[cfg(target_os = "windows")]
    {
        let drive = crate::commands::disk::drive_letter_for_index(disk_index)
            .ok_or_else(|| format!("找不到磁盘 {}", disk_index))?;
        let dev = open_volume(drive, true)?;
        dev::lock_volume(dev.0)?;
        dev::write_at(dev.0, 0, &data)?;
        dev::unlock_volume(dev.0);
        crate::app_log!("INFO", "卷 {} 引导扇区已从 {} 恢复", drive, backup_path);
        Ok(format!("卷 {} 引导扇区已恢复（来源: {}）", drive, backup_path))
    }
}

/// 创建磁盘镜像：按 4MB 块读取卷数据写入文件，推送 image-progress 事件
#[tauri::command]
pub async fn create_disk_image<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    disk_index: u32,
    output_path: String,
    skip_bad_sectors: bool,
) -> Result<String, String> {
    use tauri::Emitter;
    crate::app_log!("INFO", "create_disk_image: 磁盘 {} -> {} (跳过坏扇区={})", disk_index, output_path, skip_bad_sectors);
    #[cfg(not(target_os = "windows"))]
    { let _ = (app, disk_index, output_path, skip_bad_sectors); return Err("磁盘镜像目前仅支持 Windows".into()); }

    #[cfg(target_os = "windows")]
    {
        let drive = crate::commands::disk::drive_letter_for_index(disk_index)
            .ok_or_else(|| format!("找不到磁盘 {}", disk_index))?;

        IMAGE_CANCEL.store(false, Ordering::SeqCst);
        let out_for_msg = output_path.clone();
        std::thread::spawn(move || {
            let emit = |p: ImageProgress| { let _ = app.emit("image-progress", p); };
            let fail = |msg: String| {
                crate::app_log!("ERROR", "磁盘镜像失败: {}", msg);
                emit(ImageProgress { total_sectors: 0, copied_sectors: 0, bad_sectors: 0, speed_mbps: 0.0, status: ImageStatus::Error });
            };

            let result = (|| -> Result<String, String> {
                let dev = open_volume_no_buffering(drive)?;
                let total = volume_length(dev.0)?;
                let total_sectors = total / 512;
                probe_readable(dev.0)?;

                let mut out = std::fs::File::create(&output_path)
                    .map_err(|e| format!("创建镜像文件失败: {}", e))?;
                use std::io::Write;

                const CHUNK: u64 = 4 * 1024 * 1024; // 4MB（扇区对齐）
                let mut offset = 0u64;
                let mut bad = 0u64;
                let start = std::time::Instant::now();
                let mut buf = vec![0u8; CHUNK as usize];
                let mut last_emit = std::time::Instant::now();

                while offset < total {
                    if IMAGE_CANCEL.load(Ordering::SeqCst) {
                        return Err("用户取消了镜像任务".into());
                    }
                    let n = CHUNK.min(total - offset) as usize;
                    match read_at(dev.0, offset, n) {
                        Ok(data) => {
                            out.write_all(&data).map_err(|e| format!("写入镜像失败: {}", e))?;
                        }
                        Err(_) => {
                            if !skip_bad_sectors {
                                return Err(format!("读取失败(偏移 {})且未启用跳过坏扇区", offset));
                            }
                            bad += (n as u64) / 512;
                            buf[..n].fill(0);
                            out.write_all(&buf[..n]).map_err(|e| format!("写入镜像失败: {}", e))?;
                        }
                    }
                    offset += n as u64;
                    if last_emit.elapsed().as_millis() >= 300 || offset >= total {
                        let secs = start.elapsed().as_secs_f64().max(0.001);
                        emit(ImageProgress {
                            total_sectors,
                            copied_sectors: offset / 512,
                            bad_sectors: bad,
                            speed_mbps: (offset as f64 / (1024.0 * 1024.0)) / secs,
                            status: ImageStatus::Creating,
                        });
                        last_emit = std::time::Instant::now();
                    }
                }
                out.flush().ok();
                emit(ImageProgress {
                    total_sectors, copied_sectors: total_sectors, bad_sectors: bad,
                    speed_mbps: (total as f64 / (1024.0 * 1024.0)) / start.elapsed().as_secs_f64().max(0.001),
                    status: ImageStatus::Completed,
                });
                Ok(format!("镜像完成: {} ({} MB, 坏扇区 {} 个)", output_path, total >> 20, bad))
            })();

            match result {
                Ok(msg) => crate::app_log!("INFO", "磁盘镜像: {}", msg),
                Err(e) => fail(e),
            }
        });
        Ok(format!("镜像任务已启动: {} 盘 -> {}", drive, out_for_msg))
    }
}

/// 取消进行中的镜像任务
#[tauri::command]
pub async fn cancel_disk_image() -> Result<(), String> {
    crate::app_log!("INFO", "cancel_disk_image: 请求取消镜像任务");
    IMAGE_CANCEL.store(true, Ordering::SeqCst);
    Ok(())
}

// ===== 测试 =====
#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个带 55AA 签名的空扇区
    fn blank_sector() -> Vec<u8> {
        let mut v = vec![0u8; 512];
        v[510] = 0x55; v[511] = 0xAA;
        v
    }

    fn make_ntfs_boot() -> Vec<u8> {
        let mut s = blank_sector();
        s[0] = 0xEB; s[1] = 0x52; s[2] = 0x90;
        s[3..11].copy_from_slice(b"NTFS    ");
        s[0x0B] = 0x00; s[0x0C] = 0x08; // 2048 字节/扇区占位
        // 总扇区数 = 100000
        s[0x28..0x30].copy_from_slice(&100_000u64.to_le_bytes());
        s
    }

    fn make_fat32_boot(label: &str) -> Vec<u8> {
        let mut s = blank_sector();
        s[82..90].copy_from_slice(b"FAT32   ");
        s[0x20..0x24].copy_from_slice(&200_000u32.to_le_bytes());
        let lb = label.as_bytes();
        s[71..71 + lb.len().min(11)].copy_from_slice(&lb[..lb.len().min(11)]);
        s
    }

    #[test]
    fn mbr_parse_roundtrip() {
        let mut mbr = blank_sector();
        mbr[0..8].copy_from_slice(b"BOOTCODE");
        // 分区1: NTFS 可启动, LBA 2048, 大小 100万扇区
        let off = 446;
        mbr[off] = 0x80; mbr[off + 4] = 0x07;
        mbr[off + 8..off + 12].copy_from_slice(&2048u32.to_le_bytes());
        mbr[off + 12..off + 16].copy_from_slice(&1_000_000u32.to_le_bytes());

        let entries = parse_mbr_entries(&mbr);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].bootable);
        assert_eq!(entries[0].ptype, 0x07);
        assert_eq!(entries[0].start_lba, 2048);
        assert_eq!(entries[0].size_lba, 1_000_000);

        // 重建：保留引导代码，替换表项
        let new = build_mbr_sector(&mbr, &[
            MbrPartition { bootable: false, ptype: 0x0C, start_lba: 4096, size_lba: 500 },
            MbrPartition { bootable: true, ptype: 0x07, start_lba: 8192, size_lba: 600 },
        ]).unwrap();
        assert_eq!(&new[0..8], b"BOOTCODE");
        let re = parse_mbr_entries(&new);
        assert_eq!(re.len(), 2);
        assert_eq!(re[0].ptype, 0x0C);
        assert_eq!(re[0].start_lba, 4096);
        assert!(re[1].bootable);
        assert_eq!(re[1].size_lba, 600);
    }

    #[test]
    fn mbr_without_signature_returns_empty() {
        let mut bad = vec![0u8; 512];
        bad[446 + 4] = 0x07; // 有表项但无 55AA
        assert!(parse_mbr_entries(&bad).is_empty());
    }

    #[test]
    fn boot_signature_detection() {
        assert_eq!(detect_boot_signature(&make_ntfs_boot()), Some("NTFS"));
        assert_eq!(detect_boot_signature(&make_fat32_boot("MYDISK")), Some("FAT32"));

        let mut exfat = blank_sector();
        exfat[3..11].copy_from_slice(b"EXFAT   ");
        assert_eq!(detect_boot_signature(&exfat), Some("exFAT"));

        let mut fat16 = blank_sector();
        fat16[54..59].copy_from_slice(b"FAT16");
        assert_eq!(detect_boot_signature(&fat16), Some("FAT"));

        assert_eq!(detect_boot_signature(&blank_sector()), None);
    }

    #[test]
    fn fs_size_inference() {
        assert_eq!(fs_size_from_boot(&make_ntfs_boot(), "NTFS"), Some(100_000));
        assert_eq!(fs_size_from_boot(&make_fat32_boot("X"), "FAT32"), Some(200_000));
    }

    #[test]
    fn fat_label_extraction() {
        assert_eq!(fat_label_from_boot(&make_fat32_boot("DATA"), "FAT32"), Some("DATA".into()));
        assert_eq!(fat_label_from_boot(&make_fat32_boot("NO NAME"), "FAT32"), None);
        assert_eq!(fat_label_from_boot(&make_ntfs_boot(), "NTFS"), None);
    }

    #[test]
    fn gpt_header_and_entries() {
        let mut hdr = blank_sector();
        hdr[0..8].copy_from_slice(b"EFI PART");
        hdr[0x48..0x50].copy_from_slice(&2u64.to_le_bytes()); // 表项在 LBA2
        hdr[0x50..0x54].copy_from_slice(&128u32.to_le_bytes());
        hdr[0x54..0x58].copy_from_slice(&128u32.to_le_bytes());
        let g = parse_gpt_header(&hdr).unwrap();
        assert_eq!(g.partition_entry_lba, 2);
        assert_eq!(g.num_entries, 128);

        // 一个表项：start=2048 end=4095, name="WIN"
        let mut e = vec![0u8; 128];
        e[0] = 0x01; // type guid 非零
        e[32..40].copy_from_slice(&2048u64.to_le_bytes());
        e[40..48].copy_from_slice(&4095u64.to_le_bytes());
        let name = "WIN".encode_utf16().collect::<Vec<u16>>();
        for (i, c) in name.iter().enumerate() {
            e[56 + i * 2..58 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        let parts = parse_gpt_entries(&e, 128);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[0].end_lba, 4095);
        assert_eq!(parts[0].name, "WIN");
    }

    #[test]
    fn backup_dir_creatable() {
        // 备份目录函数应能正常创建目录
        assert!(backup_dir().is_ok());
    }
}
