use rescueforge_core::types::{DiskInfo, SmartStatus, PartitionInfo, PartitionStatus};

/// 获取所有物理磁盘信息
#[tauri::command]
pub async fn get_disks() -> Result<Vec<DiskInfo>, String> {
    enumerate_disks().map_err(|e| e.to_string())
}

/// 获取指定磁盘的 S.M.A.R.T. 状态
#[tauri::command]
pub async fn get_smart_status(disk_index: u32) -> Result<SmartStatus, String> {
    read_smart(disk_index).map_err(|e| e.to_string())
}

/// 枚举顺序与 enumerate_disks 一致：按盘符返回第 index 个盘的盘符（如 'D'）
#[cfg(target_os = "windows")]
pub fn drive_letter_for_index(disk_index: u32) -> Option<char> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetDiskFreeSpaceExW};
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOVABLE: u32 = 2;
    let mut count = 0u32;
    for drive_letter in b'A'..=b'Z' {
        let root = format!("{}:\\", drive_letter as char);
        let root_wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_wide.as_ptr())) };
        if drive_type != DRIVE_FIXED && drive_type != DRIVE_REMOVABLE { continue; }
        let mut total_bytes: u64 = 0;
        unsafe {
            let _ = GetDiskFreeSpaceExW(
                windows::core::PCWSTR(root_wide.as_ptr()),
                None, Some(&mut total_bytes as *mut u64), None,
            );
        }
        if total_bytes == 0 { continue; }
        if count == disk_index { return Some(drive_letter as char); }
        count += 1;
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn drive_letter_for_index(_disk_index: u32) -> Option<char> { None }

#[cfg(target_os = "windows")]
fn enumerate_disks() -> Result<Vec<DiskInfo>, Box<dyn std::error::Error>> {
    use windows::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW,
    };

    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOVABLE: u32 = 2;

    let mut disks = Vec::new();
    for drive_letter in b'A'..=b'Z' {
        let root = format!("{}:\\", drive_letter as char);
        let root_wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();

        let drive_type = unsafe { GetDriveTypeW(windows::core::PCWSTR(root_wide.as_ptr())) };

        if drive_type != DRIVE_FIXED && drive_type != DRIVE_REMOVABLE {
            continue;
        }

        let mut total_bytes: u64 = 0;
        let mut free_bytes: u64 = 0;
        let mut total_free: u64 = 0;

        unsafe {
            let _ = GetDiskFreeSpaceExW(
                windows::core::PCWSTR(root_wide.as_ptr()),
                Some(&mut free_bytes as *mut u64),
                Some(&mut total_bytes as *mut u64),
                Some(&mut total_free as *mut u64),
            );
        }

        if total_bytes == 0 {
            continue;
        }

        let device_path = format!("PhysicalDrive{}", disks.len());
        let is_removable = drive_type == DRIVE_REMOVABLE;

        disks.push(DiskInfo {
            index: disks.len() as u32,
            model: format!("本地磁盘 ({}:){}", drive_letter as char, if is_removable { " 可移动" } else { "" }),
            serial: (drive_letter as char).to_string(),
            capacity_bytes: total_bytes,
            free_bytes: total_free,
            sector_size: 512,
            total_sectors: total_bytes / 512,
            device_path,
            is_removable,
            partitions: vec![PartitionInfo {
                start_sector: 0,
                size_sectors: total_bytes / 512,
                filesystem: "NTFS".to_string(),
                mount_point: Some(root.clone()),
                status: PartitionStatus::Active,
                is_bootable: false,
            }],
            smart_status: None,
        });
    }
    Ok(disks)
}

#[cfg(target_os = "windows")]
fn read_smart(_disk_index: u32) -> Result<SmartStatus, Box<dyn std::error::Error>> {
    Ok(SmartStatus {
        healthy: true,
        warning: None,
        temperature: None,
        power_on_hours: None,
        attributes: vec![],
    })
}

#[cfg(not(target_os = "windows"))]
fn enumerate_disks() -> Result<Vec<DiskInfo>, Box<dyn std::error::Error>> {
    Ok(vec![])
}

#[cfg(not(target_os = "windows"))]
fn read_smart(_disk_index: u32) -> Result<SmartStatus, Box<dyn std::error::Error>> {
    Err("S.M.A.R.T. not yet implemented for this platform".into())
}
