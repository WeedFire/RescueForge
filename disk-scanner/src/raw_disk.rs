use anyhow::{Result, anyhow};
use rescueforge_core::block_device::{BlockDevice, DiskGeometry};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// 跨平台 Raw Disk 实现
pub struct RawDisk {
    file: File,
    sector_size: u64,
    total_sectors: u64,
}

impl RawDisk {
    /// 打开物理磁盘设备
    pub fn open(device_path: &str) -> Result<Self> {
        #[cfg(target_os = "windows")]
        return Self::open_windows(device_path);

        #[cfg(target_os = "linux")]
        return Self::open_linux(device_path);

        #[cfg(target_os = "macos")]
        return Self::open_macos(device_path);

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        Err(anyhow!("Unsupported operating system"))
    }

    #[cfg(target_os = "windows")]
    fn open_windows(device_path: &str) -> Result<Self> {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAG_NO_BUFFERING,
        };

        const GENERIC_READ: u32 = 0x80000000;

        let path = format!("\\\\.\\{}", device_path);
        let wide_path: Vec<u16> = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(Some(0))
            .collect();

        unsafe {
            let handle = CreateFileW(
                PCWSTR(wide_path.as_ptr()),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_NO_BUFFERING,
                None,
            )?;

            if handle.is_invalid() {
                return Err(anyhow!("Cannot open physical disk. Admin privileges required."));
            }

            Ok(Self {
                file: File::from_raw_handle(handle.0 as _),
                sector_size: 512,
                total_sectors: 10_000_000, // placeholder
            })
        }
    }

    #[cfg(target_os = "linux")]
    fn open_linux(device_path: &str) -> Result<Self> {
        let path = format!("/dev/{}", device_path);
        let file = File::open(&path)?;
        Ok(Self {
            file,
            sector_size: 512,
            total_sectors: 10_000_000,
        })
    }

    #[cfg(target_os = "macos")]
    fn open_macos(device_path: &str) -> Result<Self> {
        let path = format!("/dev/r{}", device_path);
        let file = File::open(&path)?;
        Ok(Self {
            file,
            sector_size: 512,
            total_sectors: 10_000_000,
        })
    }

    /// 读取扇区
    pub fn read_sectors(&mut self, start_sector: u64, count: u64, buffer: &mut [u8]) -> Result<()> {
        let offset = start_sector * self.sector_size;
        let bytes_to_read = count * self.sector_size;

        if buffer.len() < bytes_to_read as usize {
            return Err(anyhow!("Buffer too small"));
        }

        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut buffer[..bytes_to_read as usize])?;
        Ok(())
    }
}

impl BlockDevice for RawDisk {
    fn read_sectors(&mut self, start_sector: u64, count: u64, buffer: &mut [u8]) -> Result<()> {
        self.read_sectors(start_sector, count, buffer)
    }

    fn get_geometry(&self) -> Result<DiskGeometry> {
        Ok(DiskGeometry {
            sector_size: self.sector_size,
            total_sectors: self.total_sectors,
            capacity_bytes: self.sector_size * self.total_sectors,
            cylinders: 0,
            tracks_per_cylinder: 0,
            sectors_per_track: 0,
        })
    }
}
