//! NTFS 卷级「已删除文件」扫描与恢复（设计文档 Level-1：MFT 元数据恢复）。
//!
//! 直接以只读方式打开卷设备 `\\.\X:`（需管理员），解析引导扇区定位 $MFT，
//! 遍历 MFT 记录，收集「已删除」（in-use 位被清除）的文件条目：
//! 文件名/大小取自 $FILE_NAME，数据按 $DATA 的簇运行（data runs）从卷上直读恢复。
//! 非 Windows / 非 NTFS / 无权限时静默降级（返回空结果 + 原因说明）。

use crate::scheduler::CancellationToken;

use super::scan::FoundFile;

/// 已删除文件的虚拟路径编码：ntfs://{盘符}:/{MFT记录号}
pub const NTFS_PREFIX: &str = "ntfs://";

/// 回收站清空后残留的 `$R` 数据文件编码：recycleresidue://{盘符}:/{MFT记录号}
/// 与 ntfs:// 同是「按 MFT 记录号读数据」，但语义上表示来自回收站被清空的残留，
/// 恢复时直接按记录号读 `$R` 数据区（不依赖 `$FILE_NAME`）。
pub const RECYCLE_RESIDUE_PREFIX: &str = "recycleresidue://";

/// 判断文件完整路径是否位于目标目录内（含子目录）。Windows 路径大小写不敏感。
/// 例：`D:\temp\sub\a.txt` 位于 `D:\temp` 内 → true。
pub fn path_under_dir(full_path: &str, target_dir: &str) -> bool {
    let t = target_dir.trim_end_matches(['\\', '/']).to_lowercase();
    if t.is_empty() { return true; }
    // 取文件所在目录（去掉最后一段文件名）后比较
    let dir = match full_path.rfind(['\\', '/']) {
        Some(i) => full_path[..i].to_lowercase(),
        None => return false,
    };
    dir == t || dir.starts_with(&format!("{}\\", t))
}

/// 整卷扫描时最多返回的已删除文件数（防止历史删除记录过多撑爆前端）
const MAX_DELETED_FILES: usize = 200_000;
/// 指定目标目录过滤时的上限。需显著大于整卷上限：过滤后结果虽少，但扫描必须走完
/// 整个 MFT 才能覆盖高记录号区（最近删除的文件），否则会提前中断。
const MAX_FILTERED_FILES: usize = 500_000;

pub struct DeletedScanOutcome {
    pub files: Vec<FoundFile>,
    /// 未执行扫描的原因（用于前端提示），成功为 None
    pub note: Option<String>,
}

pub enum DeletedScanEvent<'a> {
    Batch(&'a [FoundFile]),
    /// 携带实时分析进度（0~100，MFT 已分析百分比），用于前端进度条
    Progress(u32, String),
}

// ===== 纯解析函数（跨平台，可单测） =====

pub(super) struct BootInfo {
    pub cluster_size: u64,
    pub record_size: u64,
    pub mft_offset: u64,
}

fn u16le(b: &[u8], o: usize) -> u16 {
    if o + 2 <= b.len() { u16::from_le_bytes([b[o], b[o + 1]]) } else { 0 }
}
fn u32le(b: &[u8], o: usize) -> u32 {
    if o + 4 <= b.len() { u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) } else { 0 }
}
fn u64le(b: &[u8], o: usize) -> u64 {
    if o + 8 <= b.len() {
        u64::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3], b[o + 4], b[o + 5], b[o + 6], b[o + 7]])
    } else { 0 }
}

/// 解析 NTFS 引导扇区（BPB），非 NTFS 返回 None
pub(super) fn parse_boot_sector(b: &[u8]) -> Option<BootInfo> {
    if b.len() < 512 || &b[3..8] != b"NTFS " { return None; }
    if b[510] != 0x55 || b[511] != 0xAA { return None; }
    let bytes_per_sector = u16le(b, 11) as u64;
    let sectors_per_cluster = b[13] as u64;
    if bytes_per_sector == 0 || sectors_per_cluster == 0 { return None; }
    let cluster_size = bytes_per_sector * sectors_per_cluster;
    let mft_lcn = u64le(b, 48);
    let rec_code = b[64] as i8;
    let record_size = if rec_code < 0 {
        1u64 << (-(rec_code as i32)) as u64
    } else {
        rec_code as u64 * cluster_size
    };
    if record_size < 512 || record_size > 65536 { return None; }
    let mft_offset = mft_lcn * cluster_size;
    Some(BootInfo { cluster_size, record_size, mft_offset })
}

/// 解析 NTFS 数据运行（mapping pairs）。返回按 VCN 顺序的 (起始簇, 簇数)，
/// `u64::MAX` 表示稀疏段（无实际簇，恢复时写零）。
pub(super) fn parse_runs(data: &[u8]) -> Vec<(u64, u64)> {
    let mut runs = Vec::new();
    let mut i = 0usize;
    let mut lcn: i64 = 0;
    while i < data.len() {
        let head = data[i];
        if head == 0 { break; }
        let len_b = (head & 0x0F) as usize;
        let off_b = ((head >> 4) & 0x0F) as usize;
        if len_b == 0 || i + 1 + len_b + off_b > data.len() { break; }
        let mut count: u64 = 0;
        for k in 0..len_b {
            count |= (data[i + 1 + k] as u64).checked_shl(8 * k as u32).unwrap_or(0);
        }
        let mut delta: i64 = 0;
        if off_b > 0 {
            let mut v: u64 = 0;
            for k in 0..off_b {
                v |= (data[i + 1 + len_b + k] as u64).checked_shl(8 * k as u32).unwrap_or(0);
            }
            if off_b < 8 && data[i + 1 + len_b + off_b - 1] & 0x80 != 0 {
                v |= !0u64 << (8 * off_b);
            }
            delta = v as i64;
        }
        lcn += delta;
        if off_b == 0 || lcn < 0 {
            runs.push((u64::MAX, count));
            if lcn < 0 { lcn = 0; }
        } else {
            runs.push((lcn as u64, count));
        }
        i += 1 + len_b + off_b;
    }
    runs
}

pub(super) struct NameInfo {
    pub name: String,
    pub namespace: u8,
    pub size: u64,
    pub mtime_unix: u64,
    /// 父目录的 MFT 记录号（$FILE_NAME 偏移 0x00 的低 6 字节）。
    /// 用于重建文件完整路径，实现「只扫描指定目录下已删除文件」的过滤。
    pub parent_ref: u64,
}

pub(super) fn parse_file_name_value(v: &[u8]) -> Option<NameInfo> {
    if v.len() < 0x42 { return None; }
    // $FILE_NAME 布局：0x00 父目录引用(8B: 6B 记录号 + 2B 序列号) / 0x10 修改时间 /
    // 0x30 实际大小 / 0x40 文件名长度(字符数) / 0x41 命名空间 / 0x42 文件名(UTF-16LE)
    let parent_raw = u64le(v, 0x00);
    let parent_ref = parent_raw & 0x0000_FFFF_FFFF_FFFF;
    let size = u64le(v, 0x30);
    let mtime = u64le(v, 0x10);
    let nlen = v[0x40] as usize;
    let ns = v[0x41];
    if v.len() < 0x42 + nlen * 2 { return None; }
    let mut wide = Vec::with_capacity(nlen);
    for i in 0..nlen { wide.push(u16le(v, 0x42 + i * 2)); }
    Some(NameInfo {
        name: String::from_utf16_lossy(&wide),
        namespace: ns,
        size,
        mtime_unix: filetime_to_unix(mtime),
        parent_ref,
    })
}

fn filetime_to_unix(ft: u64) -> u64 {
    let secs = ft / 10_000_000;
    secs.saturating_sub(11_644_473_600)
}

const ATTR_ATTR_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;

pub(super) struct AttrRef {
    pub attr_type: u32,
    pub start: usize,
    pub len: usize,
    pub nonresident: bool,
    pub flags: u16,
}

pub(super) fn iter_attrs(rec: &[u8]) -> Vec<AttrRef> {
    let mut out = Vec::new();
    if rec.len() < 0x30 || &rec[0..4] != b"FILE" { return out; }
    let mut off = u16le(rec, 0x14) as usize;
    while off + 8 <= rec.len() {
        let t = u32le(rec, off);
        if t == 0xFFFF_FFFF || t == 0 { break; }
        let len = u32le(rec, off + 4) as usize;
        if len < 8 || off + len > rec.len() { break; }
        let nonresident = rec[off + 8] != 0;
        let flags = u16le(rec, off + 0x0C);
        out.push(AttrRef { attr_type: t, start: off, len, nonresident, flags });
        off += len;
    }
    out
}

pub(super) fn attr_value<'a>(rec: &'a [u8], a: &AttrRef) -> Option<&'a [u8]> {
    if a.nonresident || a.start + 0x18 > rec.len() { return None; }
    let vlen = u32le(rec, a.start + 0x10) as usize;
    let voff = u16le(rec, a.start + 0x14) as usize;
    let s = a.start.checked_add(voff)?;
    if s.checked_add(vlen)? > rec.len() { return None; }
    Some(&rec[s..s + vlen])
}

pub(super) struct ParsedRecord {
    pub in_use: bool,
    pub names: Vec<NameInfo>,
    pub resident_data: Option<Vec<u8>>,
    pub data_runs: Vec<(u64, u64)>,
    pub data_size: u64,
    pub compressed: bool,
    pub attr_list_present: bool,
    pub attr_list_resident: Option<Vec<u8>>,
    pub attr_list_runs: Vec<(u64, u64)>,
    pub attr_list_size: u64,
}

pub(super) fn parse_record(rec: &[u8]) -> Option<ParsedRecord> {
    if rec.len() < 0x30 || &rec[0..4] != b"FILE" { return None; }
    let flags = u16le(rec, 0x16);
    let mut p = ParsedRecord {
        in_use: flags & 0x01 != 0,
        names: Vec::new(),
        resident_data: None,
        data_runs: Vec::new(),
        data_size: 0,
        compressed: false,
        attr_list_present: false,
        attr_list_resident: None,
        attr_list_runs: Vec::new(),
        attr_list_size: 0,
    };
    for a in iter_attrs(rec) {
        match a.attr_type {
            ATTR_FILE_NAME if !a.nonresident => {
                if let Some(v) = attr_value(rec, &a) {
                    if let Some(n) = parse_file_name_value(v) {
                        p.data_size = p.data_size.max(n.size);
                        p.names.push(n);
                    }
                }
            }
            ATTR_DATA if a.nonresident => {
                if a.len < 0x40 { continue; }
                // 非常驻属性头：0x20 = mapping pairs 偏移，0x30 = 真实数据大小（0x28 分配/0x38 初始化）
                let mpo = u16le(rec, a.start + 0x20) as usize;
                if mpo < a.len {
                    p.data_runs = parse_runs(&rec[a.start + mpo..a.start + a.len]);
                }
                p.data_size = u64le(rec, a.start + 0x30);
                // 压缩(0x0001) 或加密(0x4000) 暂不支持
                if a.flags & 0x0001 != 0 || a.flags & 0x4000 != 0 { p.compressed = true; }
            }
            ATTR_DATA => {
                if let Some(v) = attr_value(rec, &a) {
                    p.resident_data = Some(v.to_vec());
                    p.data_size = v.len() as u64;
                }
            }
            ATTR_ATTR_LIST => {
                p.attr_list_present = true;
                if a.nonresident && a.len >= 0x40 {
                    let mpo = u16le(rec, a.start + 0x20) as usize;
                    if mpo < a.len {
                        p.attr_list_runs = parse_runs(&rec[a.start + mpo..a.start + a.len]);
                    }
                    p.attr_list_size = u64le(rec, a.start + 0x30);
                } else if let Some(v) = attr_value(rec, &a) {
                    p.attr_list_resident = Some(v.to_vec());
                }
            }
            _ => {}
        }
    }
    Some(p)
}

/// 解析 $ATTRIBUTE_LIST，返回 (属性类型, 基准MFT记录号, 起始VCN)
pub(super) fn parse_attr_list_refs(buf: &[u8]) -> Vec<(u32, u64, u64)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 0x20 <= buf.len() {
        let t = u32le(buf, off);
        if t == 0 || t == 0xFFFF_FFFF { break; }
        let len = u16le(buf, off + 4) as usize;
        if len < 0x18 || off + len > buf.len() { break; }
        let vcn = u64le(buf, off + 8);
        let base = u64le(buf, off + 0x10) & 0x0000_FFFF_FFFF_FFFF;
        out.push((t, base, vcn));
        off += (len + 7) & !7;
    }
    out
}

/// Unix 时间戳 -> "YYYY-MM-DD HH:MM:SS"（UTC，civil-from-days 算法）
pub(super) fn unix_to_string(ts: u64) -> Option<String> {
    if ts == 0 { return None; }
    let days = (ts / 86400) as i64;
    let secs = ts % 86400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 { y += 1; }
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60))
}

fn sanitize_filename(s: &str) -> String {
    s.chars().map(|c| match c {
        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
        _ => c,
    }).collect()
}

// ===== Windows 卷访问实现 =====

#[cfg(windows)]
mod win {
    use super::*;
    use std::path::{Path, PathBuf};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetVolumeInformationW, ReadFile, SetFilePointerEx, FILE_BEGIN,
        FILE_FLAG_RANDOM_ACCESS, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    /// CTL_CODE(FILE_DEVICE_FILE_SYSTEM=9, 7, METHOD_BUFFERED, FILE_ANY_ACCESS) = 0x0009001C
    const FSCTL_ALLOW_EXTENDED_DASD_IO: u32 = 0x0009_001C;

    fn read_at_exact(h: HANDLE, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        unsafe {
            SetFilePointerEx(h, offset as i64, None, FILE_BEGIN)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        }
        let mut got = 0usize;
        while got < len {
            let mut r = 0u32;
            unsafe {
                ReadFile(h, Some(&mut buf[got..]), Some(&mut r), None)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            }
            if r == 0 { break; }
            got += r as usize;
        }
        if got < len {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "卷读取短读"));
        }
        Ok(buf)
    }

    fn get_volume_fs_name(drive: char) -> Result<String, String> {
        let root: Vec<u16> = format!("{}:\\", drive).encode_utf16().chain(std::iter::once(0)).collect();
        let mut fs = [0u16; 64];
        unsafe {
            GetVolumeInformationW(
                PCWSTR::from_raw(root.as_ptr()), None, None, None, None, Some(&mut fs[..]),
            ).map_err(|e| format!("无法获取卷 {} 信息: {}", drive, e))?;
        }
        let end = fs.iter().position(|&c| c == 0).unwrap_or(fs.len());
        Ok(String::from_utf16_lossy(&fs[..end]))
    }

    /// 回收站清空后残留的 `$R` / `$I` 索引（按文件名后缀配对）。
    pub struct RecycleResidue {
        /// 后缀 -> ($R 记录号, resident数据, 数据runs, 大小)
        pub r_map: std::collections::HashMap<String, (u64, Option<Vec<u8>>, Vec<(u64, u64)>, u64)>,
        /// 后缀 -> ($I 记录号, 删除前原始完整路径)
        pub i_map: std::collections::HashMap<String, (u64, String)>,
    }

    impl RecycleResidue {
        pub fn new() -> Self {
            Self { r_map: std::collections::HashMap::new(), i_map: std::collections::HashMap::new() }
        }
    }

    pub struct Volume {
        h: HANDLE,
        pub cluster_size: u64,
        pub record_size: u64,
        /// $MFT 数据运行（按 VCN 顺序）：(起始簇, 簇数)
        pub mft_runs: Vec<(u64, u64)>,
        pub mft_size: u64,
        /// 路径重建缓存：MFT 记录号 -> (目录名, 父记录号)。
        /// None 表示该记录不可用作目录（读取/解析失败），避免重复读盘。
        dir_cache: std::collections::HashMap<u64, Option<(String, u64)>>,
    }

    impl Drop for Volume {
        fn drop(&mut self) {
            unsafe { let _ = CloseHandle(self.h); }
        }
    }

    impl Volume {
        pub fn open(drive: char) -> Result<Volume, String> {
            let fs_name = get_volume_fs_name(drive)?;
            if !fs_name.eq_ignore_ascii_case("NTFS") {
                return Err(format!("卷 {} 的文件系统为 {}，已删除文件扫描目前仅支持 NTFS", drive, fs_name));
            }
            let wide: Vec<u16> = format!(r"\\.\{}:", drive).encode_utf16().chain(std::iter::once(0)).collect();
            let h = unsafe {
                CreateFileW(
                    PCWSTR::from_raw(wide.as_ptr()),
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_RANDOM_ACCESS,
                    None,
                )
            }.map_err(|e| format!("无法打开卷 {} (需要管理员权限): {}", drive, e))?;
            unsafe {
                let mut ret = 0u32;
                let _ = DeviceIoControl(h, FSCTL_ALLOW_EXTENDED_DASD_IO, None, 0, None, 0, Some(&mut ret), None);
            }
            let boot = read_at_exact(h, 0, 512).map_err(|e| format!("读取引导扇区失败: {}", e))?;
            let bi = parse_boot_sector(&boot).ok_or_else(|| format!("卷 {} 引导扇区不是有效 NTFS", drive))?;
            let rec0 = read_at_exact(h, bi.mft_offset, bi.record_size as usize)
                .map_err(|e| format!("读取 $MFT 首记录失败: {}", e))?;
            let p0 = parse_record(&rec0).ok_or("无法解析 $MFT 首记录")?;
            let (mft_runs, mft_size) = if !p0.data_runs.is_empty() {
                (p0.data_runs.clone(), p0.data_size)
            } else {
                // 回退：假设 $MFT 在引导扇区声明处连续存放（覆盖前 256MB）
                (vec![(bi.mft_offset / bi.cluster_size, (256u64 << 20) / bi.cluster_size)], 256u64 << 20)
            };
            let mut vol = Volume { h, cluster_size: bi.cluster_size, record_size: bi.record_size, mft_runs, mft_size, dir_cache: std::collections::HashMap::new() };
            // 大型卷的 $MFT 高度碎片化：首记录只保存第一段运行，其余分片记录在 $ATTRIBUTE_LIST 中。
            // 不跟随分片会导致扫描提前结束、漏掉绝大部分已删除记录。
            if p0.attr_list_present {
                match vol.extend_mft_runs_from_attr_list(&p0) {
                    Ok(n) => crate::app_log!("INFO", "卷 {}: $MFT 分片合并完成，共 {} 段运行", drive, n),
                    Err(e) => crate::app_log!("WARN", "卷 {}: $MFT 分片合并失败（仅扫描首段）: {}", drive, e),
                }
            }
            crate::app_log!("INFO", "卷 {} 打开成功: 簇={}B MFT记录={}B MFT大小={}MB",
                drive, vol.cluster_size, vol.record_size, vol.mft_size >> 20);
            Ok(vol)
        }

        /// 跟随 $MFT 首记录的 $ATTRIBUTE_LIST，把所有 $DATA 分片的运行按 VCN 顺序合并进 mft_runs
        fn extend_mft_runs_from_attr_list(&mut self, p0: &ParsedRecord) -> Result<usize, String> {
            let list = if let Some(v) = &p0.attr_list_resident {
                v.clone()
            } else if !p0.attr_list_runs.is_empty() {
                self.read_run_data(&p0.attr_list_runs, p0.attr_list_size.min(4 << 20))?
            } else {
                return Err("属性列表无数据".into());
            };
            // (起始VCN, 基准记录号)，按 VCN 排序后逐段拼接，跳过首记录自身段（p0.data_runs 已含）
            let mut refs: Vec<(u64, u64)> = parse_attr_list_refs(&list)
                .into_iter()
                .filter(|(t, _, _)| *t == ATTR_DATA)
                .map(|(_, base, vcn)| (vcn, base))
                .collect();
            refs.sort();
            let mut runs: Vec<(u64, u64)> = p0.data_runs.clone();
            let mut seen: Vec<u64> = Vec::new();
            for (_, base) in refs {
                if base == 0 || seen.contains(&base) { continue; }
                seen.push(base);
                let rec = match self.read_mft_record(base) { Ok(r) => r, Err(_) => continue };
                if let Some(ep) = parse_record(&rec) {
                    runs.extend(ep.data_runs);
                }
            }
            if runs.is_empty() { return Err("未找到 $MFT 数据分片".into()); }
            self.mft_runs = runs;
            Ok(self.mft_runs.len())
        }

        fn vcn_to_lcn(&self, vcn: u64) -> Result<u64, String> {
            let mut cur = 0u64;
            for &(start, count) in &self.mft_runs {
                if vcn >= cur && vcn < cur + count { return Ok(start + (vcn - cur)); }
                cur += count;
            }
            Err("MFT 簇映射越界".into())
        }

        pub fn read_mft_record(&self, index: u64) -> Result<Vec<u8>, String> {
            let rs = self.record_size;
            let pos = index * rs;
            if pos + rs > self.mft_size { return Err("MFT 记录越界".into()); }
            let mut buf = Vec::with_capacity(rs as usize);
            let mut fpos = pos;
            let mut remaining = rs;
            while remaining > 0 {
                let vcn = fpos / self.cluster_size;
                let in_off = fpos % self.cluster_size;
                let lcn = self.vcn_to_lcn(vcn)?;
                let chunk = (self.cluster_size - in_off).min(remaining);
                if lcn == u64::MAX {
                    buf.extend(std::iter::repeat(0u8).take(chunk as usize));
                } else {
                    let b = read_at_exact(self.h, lcn * self.cluster_size + in_off, chunk as usize)
                        .map_err(|e| format!("读取 MFT 记录 {} 失败: {}", index, e))?;
                    buf.extend_from_slice(&b);
                }
                fpos += chunk;
                remaining -= chunk;
            }
            Ok(buf)
        }

        /// 重建某条记录所属目录的完整路径（不含文件名），如 `D:\temp\sub`。
        /// 沿父目录引用链向上递归到根目录（MFT 记录号 5），逐段拼接。
        /// 结果缓存在 `dir_cache`，避免同一父目录被反复读盘。
        /// 父目录链断裂（记录已被覆写/损坏）时返回 None，调用方据此跳过该文件。
        pub fn build_dir_path(&mut self, parent: u64, drive: char) -> Option<String> {
            const ROOT_RECORD: u64 = 5;
            const MAX_DEPTH: usize = 128;
            let mut parts: Vec<String> = Vec::new();
            let mut cur = parent;
            for _ in 0..MAX_DEPTH {
                if cur == ROOT_RECORD || cur == 0 { break; }
                let entry = match self.dir_cache.get(&cur) {
                    Some(e) => e.clone(),
                    None => {
                        let e = self.read_mft_record(cur).ok()
                            .and_then(|rec| parse_record(&rec))
                            .and_then(|p| {
                                p.names.iter().find(|n| n.namespace == 1)
                                    .or_else(|| p.names.iter().find(|n| n.namespace == 0))
                                    .or_else(|| p.names.iter().find(|n| n.namespace == 3))
                                    .map(|n| (n.name.clone(), n.parent_ref))
                            });
                        // 目录记录可能已损坏或被覆写，缓存失败结果避免每次重试读盘
                        self.dir_cache.insert(cur, e.clone());
                        e
                    }
                };
                match entry {
                    Some((name, p)) => { parts.push(name); cur = p; }
                    None => return None,
                }
            }
            parts.reverse();
            if parts.is_empty() {
                Some(format!("{}:\\", drive))
            } else {
                Some(format!("{}:\\{}", drive, parts.join("\\")))
            }
        }

        /// 收集回收站被清空后残留的 `$R` / `$I` 记录。
        ///
        /// 清空回收站只是把 `$R`(数据载体) 与 `$I`(元数据) 标记为删除，**数据并未擦除**：
        /// - `$I` 内容里保存着文件的原始完整路径与文件名；
        /// - `$R` 的簇里仍是原文件内容。
        /// 二者按文件名后缀配对（`$IABC` ↔ `$RABC`）即可还原文件。
        /// 注意：这两类文件都以 `$` 开头，会被 `deleted_candidate` 过滤掉，故单独收集。
        fn collect_recycle_residue(&mut self, rec: &[u8], idx: u64, res: &mut RecycleResidue) {
            if rec.len() < 0x18 || &rec[0..4] != b"FILE" { return; }
            let flags = u16le(rec, 0x16);
            if flags & 0x01 != 0 { return; } // 只收已删除的
            let p = match parse_record(rec) { Some(p) => p, None => return };
            if p.compressed { return; }
            let name = match p.names.iter().find(|n| n.namespace == 1)
                .or_else(|| p.names.iter().find(|n| n.namespace == 0))
                .or_else(|| p.names.iter().find(|n| n.namespace == 3)) { Some(n) => n, None => return };

            let (kind, suffix) = if let Some(s) = name.name.strip_prefix("$R") {
                if s.len() < 3 { return; }
                ('R', s.to_string())
            } else if let Some(s) = name.name.strip_prefix("$I") {
                if s.len() < 3 { return; }
                ('I', s.to_string())
            } else {
                return;
            };

            // 读记录数据：resient 直接取；非常驻则按 runs 读（$I 很小，限制 4KB 足够）
            let data: Option<Vec<u8>> = if let Some(v) = &p.resident_data {
                if v.is_empty() { None } else { Some(v.clone()) }
            } else if !p.data_runs.is_empty() {
                let want = p.data_size.max(1).min(4096);
                self.read_run_data(&p.data_runs, want).ok()
            } else {
                None
            };
            let size = p.data_size.max(name.size);

            match kind {
                'I' => {
                    // $I 元数据 → 解出删除前的原始完整路径
                    if let Some(d) = data {
                        if let Some((orig, _)) = crate::commands::scan::parse_recycle_info_meta(&d) {
                            if !orig.is_empty() {
                                res.i_map.insert(suffix, (idx, orig));
                            }
                        }
                    }
                }
                'R' => {
                    if size == 0 { return; }
                    res.r_map.insert(suffix, (idx, data.clone(), p.data_runs.clone(), size));
                }
                _ => {}
            }
        }

        /// 读任意 MFT 记录的数据内容（最多 max_len 字节），用于取样与恢复
        pub fn read_record_data(&mut self, idx: u64, max_len: u64) -> Result<Vec<u8>, String> {
            let rec = self.read_mft_record(idx)?;
            let p = parse_record(&rec).ok_or_else(|| "MFT 记录无效".to_string())?;
            if let Some(v) = &p.resident_data {
                let n = (v.len() as u64).min(max_len) as usize;
                return Ok(v[..n].to_vec());
            }
            if p.attr_list_present {
                let runs = self.collect_extent_runs(idx, &p)?;
                return self.read_run_data(&runs, max_len);
            }
            if p.data_runs.is_empty() {
                return Err("该记录没有数据运行".into());
            }
            self.read_run_data(&p.data_runs, max_len)
        }

        pub fn read_run_data(&self, runs: &[(u64, u64)], max_len: u64) -> Result<Vec<u8>, String> {
            let mut out = Vec::new();
            for &(lcn, count) in runs {
                if out.len() as u64 >= max_len { break; }
                let want = (count * self.cluster_size).min(max_len - out.len() as u64) as usize;
                if lcn == u64::MAX {
                    out.extend(std::iter::repeat(0u8).take(want));
                } else {
                    let buf = read_at_exact(self.h, lcn * self.cluster_size, want)
                        .map_err(|e| format!("{}", e))?;
                    out.extend_from_slice(&buf);
                }
            }
            Ok(out)
        }

        /// 跟随 $ATTRIBUTE_LIST 收集所有 $DATA 分片的运行
        fn collect_extent_runs(&mut self, base_idx: u64, p: &ParsedRecord) -> Result<Vec<(u64, u64)>, String> {
            let list = if let Some(v) = &p.attr_list_resident {
                v.clone()
            } else if !p.attr_list_runs.is_empty() {
                self.read_run_data(&p.attr_list_runs, p.attr_list_size)?
            } else {
                return Err("属性列表无数据".into());
            };
            let mut refs: Vec<(u32, u64, u64)> = parse_attr_list_refs(&list)
                .into_iter().filter(|(t, _, _)| *t == ATTR_DATA).collect();
            refs.sort_by_key(|(_, _, vcn)| *vcn);
            let mut runs: Vec<(u64, u64)> = p.data_runs.clone();
            let mut seen: Vec<u64> = Vec::new();
            for (_, base, _) in refs {
                if base == base_idx || base == 0 || seen.contains(&base) { continue; }
                seen.push(base);
                let rec = self.read_mft_record(base)?;
                if let Some(ep) = parse_record(&rec) {
                    runs.extend(ep.data_runs);
                }
            }
            if runs.is_empty() { Err("未找到数据分片".into()) } else { Ok(runs) }
        }

        /// 判断一条已删除的 MFT 记录是否可作为可恢复文件。
        /// `filter` 为 Some(目录) 时，只保留该目录（含子目录）下的文件。
        fn deleted_candidate(&mut self, rec: &[u8], idx: u64, drive: char, counter: &mut u64, filter: Option<&str>) -> Option<FoundFile> {
            // 快速短路：绝大多数 MFT 记录是 in-use（现存文件），只需读记录头 2 字节 flags
            // 即可跳过，避免对它们做完整属性遍历（parse_record 会解析所有属性含 $DATA
            // mapping pairs，是 MFT 全量扫描最主要的耗时点）。in-use 标志 = flags & 0x01。
            if rec.len() < 0x18 || &rec[0..4] != b"FILE" { return None; }
            let flags = u16le(rec, 0x16);
            if flags & 0x01 != 0 { return None; }
            let p = parse_record(rec)?;
            if p.in_use || p.compressed { return None; }
            // 选最佳文件名：Win32 > POSIX > Win32&DOS，跳过 DOS 短名与 $ 系统文件
            let name = p.names.iter().find(|n| n.namespace == 1)
                .or_else(|| p.names.iter().find(|n| n.namespace == 0))
                .or_else(|| p.names.iter().find(|n| n.namespace == 3))?;
            if name.name.is_empty() || name.name.starts_with('$') { return None; }

            // 重建删除前的完整路径：沿父目录引用链上溯。
            let original_path = self.build_dir_path(name.parent_ref, drive)
                .map(|d| if d.ends_with('\\') { format!("{}{}", d, name.name) }
                         else { format!("{}\\{}", d, name.name) });
            if let Some(target) = filter {
                // 只在「能确定路径且明确不在目标目录内」时才过滤掉。
                // 路径重建失败（父目录记录已被覆写/损坏）一律保留：用户删除整个目录时
                // 父目录记录同样不可用，若此时丢弃会导致目标文件全部扫不出来。
                if let Some(full) = &original_path {
                    if !path_under_dir(full, target) { return None; }
                }
            }

            let mut runs: Vec<(u64, u64)> = Vec::new();
            let mut head: Vec<u8> = Vec::new();
            if let Some(v) = &p.resident_data {
                if v.is_empty() { return None; }
                head = v.iter().take(32).copied().collect();
            } else if p.attr_list_present {
                runs = self.collect_extent_runs(idx, &p).ok()?;
            } else if !p.data_runs.is_empty() {
                runs = p.data_runs.clone();
            } else {
                return None; // 无 $DATA（目录等），跳过
            }
            if runs.is_empty() && head.is_empty() { return None; }
            if head.is_empty() {
                head = self.read_run_data(&runs, 32).unwrap_or_default();
            }
            let size = p.data_size.max(name.size);
            if size == 0 { return None; }
            let file_type = if head.is_empty() {
                "Unknown".to_string()
            } else {
                crate::commands::scan::match_signature(&head).to_string()
            };
            // 数据簇被覆写/清零的典型特征：文件头全为 0。MFT 记录还在（所以能被扫出来），
            // 但数据区已不是原内容，恢复出来必然是空文件或打不开。扫描阶段就标记出来，
            // 避免用户满怀希望地恢复一堆救不回来的文件。
            let data_intact = if head.is_empty() {
                None
            } else {
                Some(!head.iter().all(|&b| b == 0))
            };
            *counter += 1;
            Some(FoundFile {
                id: format!("f-{}", *counter),
                filename: name.name.clone(),
                filepath: format!("{}{}:/{}", NTFS_PREFIX, drive, idx),
                extension: std::path::Path::new(&name.name)
                    .extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
                size_bytes: size,
                file_type,
                modified_time: unix_to_string(name.mtime_unix),
                deleted: true,
                original_path,
                data_intact,
            })
        }
    }

    /// `target_dir` 为 Some 时只返回该目录下的已删除文件（如 `D:\temp`）；None 返回整卷。
    pub fn scan_volume_deleted(
        drive: char,
        target_dir: Option<&str>,
        counter: &mut u64,
        token: &CancellationToken,
        on_event: &mut dyn FnMut(DeletedScanEvent),
    ) -> DeletedScanOutcome {
        if let Some(t) = target_dir {
            crate::app_log!("INFO", "卷 {} 已删除文件扫描（限定目录 {}）启动", drive, t);
        }
        let mut vol = match Volume::open(drive) {
            Ok(v) => v,
            Err(e) => {
                let lower = e.to_lowercase();
                let note = if e.contains("拒绝") || lower.contains("denied") || lower.contains("0x80070005") {
                    format!("卷 {} 需要管理员权限才能扫描已删除文件，请以管理员身份运行本程序", drive)
                } else { e };
                crate::app_log!("WARN", "卷 {} 已删除文件扫描不可用: {}", drive, note);
                return DeletedScanOutcome { files: vec![], note: Some(note) };
            }
        };
        let record_size = vol.record_size.max(1024);
        let mft_size = vol.mft_size;
        let runs = vol.mft_runs.clone();
        let mut files: Vec<FoundFile> = Vec::new();
        let mut pending: Vec<FoundFile> = Vec::new();
        let mut residue = RecycleResidue::new();
        let mut pos: u64 = 0;
        const CHUNK: u64 = 16 * 1024 * 1024;

        'outer: for &(lcn, count) in &runs {
            let run_bytes = count * vol.cluster_size;
            let mut off = 0u64;
            while off < run_bytes && pos < mft_size {
                if !token.wait_while_paused() || token.is_cancelled() { break 'outer; }
                let n = CHUNK.min(run_bytes - off).min(mft_size - pos);
                let buf = if lcn == u64::MAX {
                    vec![0u8; n as usize]
                } else {
                    match read_at_exact(vol.h, lcn * vol.cluster_size + off, n as usize) {
                        Ok(b) => b,
                        Err(_) => { pos += n; off += n; continue; }
                    }
                };
                let mut i = 0usize;
                while i + record_size as usize <= buf.len() {
                    let rec_idx = (pos + i as u64) / record_size;
                    // 0~23 为系统元文件记录，跳过
                    if rec_idx >= 24 {
                        let rec = &buf[i..i + record_size as usize];
                        if let Some(f) = vol.deleted_candidate(rec, rec_idx, drive, counter, target_dir) {
                            pending.push(f);
                            if pending.len() >= 50 {
                                let batch: Vec<FoundFile> = pending.drain(..).collect();
                                files.extend(batch.iter().cloned());
                                on_event(DeletedScanEvent::Batch(&batch));
                            }
                        }
                        // 同时收集回收站清空后残留的 $R / $I（两者以 $ 开头，被 deleted_candidate 过滤）
                        vol.collect_recycle_residue(rec, rec_idx, &mut residue);
                    }
                    i += record_size as usize;
                }
                pos += n;
                off += n;
                // 进度推送：每跨过 1% 的 MFT 就推一次，前端进度条能实时走动，
                // 避免大卷扫描时进度长时间卡在固定值、让用户误以为卡死。
                let pct = (pos as f64 / mft_size as f64 * 100.0) as u32;
                let prev_pct = ((pos - n) as f64 / mft_size as f64 * 100.0) as u32;
                if pct != prev_pct {
                    on_event(DeletedScanEvent::Progress(pct, format!(
                        "卷 {}: 已分析 MFT {}%，发现 {} 个已删除文件",
                        drive, pct, files.len() + pending.len())));
                }
                // 上限只用于保护前端不被海量结果撑死。
                // 指定目标目录时命中结果通常远少于整卷，必须用更大的上限：
                // 若沿用整卷上限，MFT 前半段就撞线提前 break，而刚删除的文件位于
                // 高记录号区（MFT 后段），扫描根本走不到 —— 表现为「强制删除的文件扫不出来」。
                // 仍保留上限而非无限，避免扫描卷根目录（如 D:\）时结果膨胀拖垮前端。
                let limit = if target_dir.is_some() { MAX_FILTERED_FILES } else { MAX_DELETED_FILES };
                if files.len() + pending.len() >= limit {
                    on_event(DeletedScanEvent::Progress(100, format!("卷 {}: 已达 {} 条上限，停止分析", drive, limit)));
                    break 'outer;
                }
            }
        }
        if !pending.is_empty() {
            let batch: Vec<FoundFile> = pending.drain(..).collect();
            files.extend(batch.iter().cloned());
            on_event(DeletedScanEvent::Batch(&batch));
        }

        // ===== 配对回收站清空残留：$I(原名) × $R(数据) =====
        // 清空回收站只标记删除、不擦数据。$I 里是删除前的完整路径，$R 里是原内容。
        let mut residue_batch: Vec<FoundFile> = Vec::new();
        for (suffix, (r_idx, _r_head, _r_runs, r_size)) in residue.r_map.iter() {
            // 原始路径优先取自 $I；若 $I 缺失/被覆写，退化为 $R 序号命名（仍可恢复内容）
            let orig = residue.i_map.get(suffix);
            let orig_path = orig.map(|(_, p)| p.clone());
            // 目录过滤：有原始路径且明确不在目标目录内 → 跳过
            if let Some(t) = target_dir {
                if let Some(p) = &orig_path {
                    if !path_under_dir(p, t) { continue; }
                }
            }
            let filename = orig_path.as_ref()
                .and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| format!("recovered_{}.bin", suffix));
            let ext = std::path::Path::new(&filename)
                .extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            // 数据完好性：读 $R 头 32 字节判断是否被覆写（全零=已覆盖）
            let head = vol.read_record_data(*r_idx, 32).unwrap_or_default();
            let data_intact = if head.is_empty() { None } else { Some(!head.iter().all(|&b| b == 0)) };
            let file_type = if head.is_empty() {
                "Unknown".to_string()
            } else {
                crate::commands::scan::match_signature(&head).to_string()
            };
            *counter += 1;
            residue_batch.push(FoundFile {
                id: format!("f-{}", *counter),
                filename: filename.clone(),
                // 用特殊前缀标记「回收站残留 $R」，恢复时按其记录号读数据
                filepath: format!("{}{}:/{}", RECYCLE_RESIDUE_PREFIX, drive, r_idx),
                extension: ext,
                size_bytes: *r_size,
                file_type,
                modified_time: None,
                deleted: true,
                original_path: orig_path,
                data_intact,
            });
        }
        if !residue_batch.is_empty() {
            crate::app_log!("INFO", "卷 {} 回收站残留配对: {} 个 ($I={} $R={})",
                drive, residue_batch.len(), residue.i_map.len(), residue.r_map.len());
            files.extend(residue_batch.iter().cloned());
            on_event(DeletedScanEvent::Batch(&residue_batch));
        }

        crate::app_log!("INFO", "卷 {} 已删除文件扫描结束: {} 个", drive, files.len());
        DeletedScanOutcome { files, note: None }
    }

    pub fn recover_deleted_file_impl(
        encoded: &str, output_dir: &Path, display_name: Option<&str>,
    ) -> std::io::Result<PathBuf> {
        fn io_err(msg: impl Into<String>) -> std::io::Error {
            std::io::Error::new(std::io::ErrorKind::Other, msg.into())
        }
        // 兼容两种编码：ntfs:// 与 recycleresidue://（回收站清空残留的 $R），
        // 二者都按「盘符 + MFT 记录号」读取数据，仅语义不同。
        let rest = encoded.strip_prefix(NTFS_PREFIX)
            .or_else(|| encoded.strip_prefix(RECYCLE_RESIDUE_PREFIX))
            .ok_or_else(|| io_err("非法的已删除文件编码"))?;
        let (drive_part, idx_part) = rest.split_once(":/").ok_or_else(|| io_err("非法的已删除文件编码"))?;
        let drive = drive_part.chars().next().ok_or_else(|| io_err("缺少盘符"))?;
        let idx: u64 = idx_part.parse().map_err(|_| io_err("非法的 MFT 记录号"))?;

        let mut vol = Volume::open(drive).map_err(io_err)?;
        let rec = vol.read_mft_record(idx).map_err(io_err)?;
        let p = parse_record(&rec).ok_or_else(|| io_err("MFT 记录无效"))?;
        if p.in_use {
            return Err(io_err("该记录已被新文件重用，原文件已无法恢复"));
        }
        if p.compressed {
            return Err(io_err("压缩/加密文件暂不支持恢复"));
        }
        let mft_name = p.names.iter().find(|n| n.namespace == 1)
            .or_else(|| p.names.iter().find(|n| n.namespace == 0))
            .map(|n| n.name.clone())
            .unwrap_or_else(|| format!("deleted_{}.bin", idx));
        let fname = display_name.filter(|s| !s.is_empty())
            .map(sanitize_filename)
            .unwrap_or_else(|| sanitize_filename(&mft_name));

        std::fs::create_dir_all(output_dir)?;
        let dest = crate::commands::scan::unique_dest_path(output_dir, &fname)?;

        if let Some(v) = p.resident_data {
            std::fs::write(&dest, v)?;
        } else {
            let mut runs = p.data_runs.clone();
            if p.attr_list_present {
                runs = vol.collect_extent_runs(idx, &p).map_err(io_err)?;
            }
            if runs.is_empty() { return Err(io_err("该记录没有数据运行")); }
            let size = p.data_size.max(p.names.iter().map(|n| n.size).max().unwrap_or(0));
            write_runs_to_file(&vol, &runs, size, &dest)?;
        }
        crate::app_log!("INFO", "已从卷 {} MFT#{} 恢复: {}", drive, idx, dest.display());
        Ok(dest)
    }

    fn write_runs_to_file(vol: &Volume, runs: &[(u64, u64)], size: u64, dest: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(dest)?;
        let mut written = 0u64;
        for &(lcn, count) in runs {
            if written >= size { break; }
            let want = (count * vol.cluster_size).min(size - written);
            if lcn == u64::MAX {
                let zeros = vec![0u8; (want as usize).min(1 << 20)];
                let mut left = want as usize;
                while left > 0 {
                    let n = left.min(zeros.len());
                    f.write_all(&zeros[..n])?;
                    left -= n;
                }
            } else {
                let base = lcn * vol.cluster_size;
                let mut left = want as usize;
                let mut off = 0u64;
                while left > 0 {
                    let n = left.min(4 << 20);
                    let buf = read_at_exact(vol.h, base + off, n)?;
                    f.write_all(&buf)?;
                    off += n as u64;
                    left -= n;
                }
            }
            written += want;
        }
        Ok(())
    }
}

// ===== 跨平台入口 =====

#[cfg(windows)]
pub fn scan_volume_deleted(
    drive: char,
    target_dir: Option<&str>,
    counter: &mut u64,
    token: &CancellationToken,
    on_event: &mut dyn FnMut(DeletedScanEvent),
) -> DeletedScanOutcome {
    win::scan_volume_deleted(drive, target_dir, counter, token, on_event)
}

#[cfg(not(windows))]
pub fn scan_volume_deleted(
    _drive: char,
    _target_dir: Option<&str>,
    _counter: &mut u64,
    _token: &CancellationToken,
    _on_event: &mut dyn FnMut(DeletedScanEvent),
) -> DeletedScanOutcome {
    DeletedScanOutcome { files: vec![], note: None }
}

#[cfg(windows)]
pub fn recover_deleted_file(
    encoded: &str, output_dir: &std::path::Path, display_name: Option<&str>,
) -> std::io::Result<std::path::PathBuf> {
    win::recover_deleted_file_impl(encoded, output_dir, display_name)
}

#[cfg(not(windows))]
pub fn recover_deleted_file(
    _encoded: &str, _output_dir: &std::path::Path, _display_name: Option<&str>,
) -> std::io::Result<std::path::PathBuf> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "已删除文件恢复仅支持 Windows"))
}

// ===== 测试 =====
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_boot_sector_ntfs() {
        let mut b = vec![0u8; 512];
        b[3..8].copy_from_slice(b"NTFS ");
        b[11] = 0x00; b[12] = 0x02; // 512 字节/扇区
        b[13] = 8;                   // 8 扇区/簇
        b[48..56].copy_from_slice(&1000u64.to_le_bytes());
        b[64] = 0xF6;                // -10 → 1024 字节记录
        b[510] = 0x55; b[511] = 0xAA;
        let bi = parse_boot_sector(&b).unwrap();
        assert_eq!(bi.cluster_size, 4096);
        assert_eq!(bi.record_size, 1024);
        assert_eq!(bi.mft_offset, 1000 * 4096);
        assert!(parse_boot_sector(&vec![0u8; 512]).is_none());
    }

    #[test]
    fn parse_runs_single_and_negative_delta() {
        // run1: head=0x23 (len=3,off=2) count=[00,08,00]=2048, delta=[10,00]=+16 → lcn=16
        // run2: head=0x12 (len=2,off=1) count=[00,01]=256, delta=[F6]=-10 → lcn=6
        let data = [0x23, 0x00, 0x08, 0x00, 0x10, 0x00, 0x12, 0x00, 0x01, 0xF6, 0x00];
        let runs = parse_runs(&data);
        assert_eq!(runs, vec![(16, 2048), (6, 256)]);
    }

    #[test]
    fn parse_runs_sparse_run() {
        // head=0x02 (len=2,off=0)：无偏移字段即稀疏，count=[00,04]=1024
        let data = [0x02, 0x00, 0x04, 0x00];
        let runs = parse_runs(&data);
        assert_eq!(runs, vec![(u64::MAX, 1024)]);
    }

    #[test]
    fn parse_file_name_utf16() {
        let mut v = vec![0u8; 0x42];
        v[0x30..0x38].copy_from_slice(&12345u64.to_le_bytes());
        v[0x10..0x18].copy_from_slice(&132_000_000_000_000_000u64.to_le_bytes());
        v[0x40] = 4;
        v[0x41] = 1;
        for c in "test".encode_utf16() {
            v.extend_from_slice(&c.to_le_bytes());
        }
        let n = parse_file_name_value(&v).unwrap();
        assert_eq!(n.name, "test");
        assert_eq!(n.size, 12345);
        assert_eq!(n.namespace, 1);
        assert!(n.mtime_unix > 0);
    }

    #[test]
    fn parse_file_name_extracts_parent_ref() {
        // $FILE_NAME 偏移 0x00 的低 6 字节为父目录 MFT 记录号（高 2 字节是序列号，须屏蔽）
        let mut v = vec![0u8; 0x42];
        let parent_raw: u64 = 0xABCD_0000_0000_0042; // 低6字节=0x42，高2字节=0xABCD(序列号)
        v[0x00..0x08].copy_from_slice(&parent_raw.to_le_bytes());
        v[0x30..0x38].copy_from_slice(&999u64.to_le_bytes());
        v[0x40] = 1;
        v[0x41] = 1;
        let ch: u16 = 'x' as u16;
        v.extend_from_slice(&ch.to_le_bytes());
        let n = parse_file_name_value(&v).unwrap();
        assert_eq!(n.parent_ref, 0x42, "父目录记录号必须屏蔽高 2 字节序列号");
        assert_eq!(n.name, "x");
        assert_eq!(n.size, 999);
    }

    #[test]
    fn path_under_dir_matches_target_and_subdirs() {
        assert!(path_under_dir("D:\\temp\\a.txt", "D:\\temp"), "同目录文件必须匹配");
        assert!(path_under_dir("D:\\temp\\sub\\a.txt", "D:\\temp"), "子目录文件必须匹配");
        assert!(path_under_dir("d:\\TEMP\\sub\\a.txt", "D:\\temp"), "大小写不敏感");
        assert!(path_under_dir("D:\\temp\\a.txt", "D:\\temp\\"), "目标末尾带分隔符仍匹配");
        // 关键负例：D:\temp2 不能误判为 D:\temp 的子目录（前缀匹配陷阱）
        assert!(!path_under_dir("D:\\temp2\\a.txt", "D:\\temp"), "同级相似目录不得匹配");
        assert!(!path_under_dir("D:\\other\\a.txt", "D:\\temp"), "无关目录不得匹配");
        assert!(!path_under_dir("C:\\temp\\a.txt", "D:\\temp"), "其他盘符不得匹配");
    }

    /// 诊断用（需管理员）：定位「强制删除文件扫不出来」发生在哪个环节。
    /// 依次验证：卷能否打开 → 不过滤能扫到多少 → 路径重建成功率 → 过滤后剩余多少。
    /// 运行：cargo test -p rescueforge --lib diag_deleted_scan_pipeline -- --ignored --nocapture
    #[test]
    #[cfg(windows)]
    #[ignore = "诊断用，需要管理员权限"]
    fn diag_deleted_scan_pipeline() {
        // 环节1：卷能否打开（无管理员权限会在此失败并静默降级）
        match win::Volume::open('D') {
            Err(e) => { eprintln!("[DIAG-1] 卷D打开失败(权限问题?): {}", e); return; }
            Ok(v) => eprintln!("[DIAG-1] 卷D打开成功: 簇={}B 记录={}B MFT={}MB 段数={}",
                v.cluster_size, v.record_size, v.mft_size >> 20, v.mft_runs.len()),
        }
        // 环节4优先：按 D:\temp 过滤（用户真实场景），先出结果再跑整卷统计
        let token = CancellationToken::new();
        let mut counter2 = 0u64;
        let filtered = scan_volume_deleted('D', Some("D:\\temp"), &mut counter2, &token, &mut |_| {});
        eprintln!("[DIAG-4] 过滤 D:\\temp 结果 {} 个  <<< 用户关注的关键数字", filtered.files.len());
        // 输出 MFT 记录号分布：命中记录号越大说明越靠后，越能证明「走完了整个 MFT」
        let mut max_idx = 0u64;
        for f in filtered.files.iter().take(30) {
            let idx = f.filepath.rsplit('/').next()
                .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            max_idx = max_idx.max(idx);
            eprintln!("[DIAG-4]   name={} mft#={} orig={:?} intact={:?}",
                f.filename, idx, f.original_path, f.data_intact);
        }
        for f in filtered.files.iter() {
            let idx = f.filepath.rsplit('/').next()
                .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            max_idx = max_idx.max(idx);
        }
        eprintln!("[DIAG-4] 命中记录最大 MFT 号 = {}（MFT 总记录数约 {}）",
            max_idx, vol_mft_records());
        let damaged = filtered.files.iter().filter(|f| f.data_intact == Some(false)).count();
        eprintln!("[DIAG-4] 其中数据已被覆写 {} / {}", damaged, filtered.files.len());

        // 环节2：不过滤整卷扫描（会因 20 万上限提前停止，仅作总量参考）
        let mut counter = 0u64;
        let all = scan_volume_deleted('D', None, &mut counter, &token, &mut |_| {});
        eprintln!("[DIAG-2] 不过滤整卷(受20万上限约束): {} 个, note={:?}", all.files.len(), all.note);
        let rebuilt = all.files.iter().filter(|f| f.original_path.is_some()).count();
        eprintln!("[DIAG-3] 路径重建成功 {} / 总数 {}", rebuilt, all.files.len());
    }

    /// 诊断辅助：估算卷 D 的 MFT 总记录数
    fn vol_mft_records() -> u64 {
        match win::Volume::open('D') {
            Ok(v) => v.mft_size / v.record_size,
            Err(_) => 0,
        }
    }

    #[test]
    fn parse_attr_list_refs_entries() {
        let mut buf = vec![0u8; 0x28];
        buf[0..4].copy_from_slice(&ATTR_DATA.to_le_bytes());
        buf[4..6].copy_from_slice(&0x20u16.to_le_bytes());
        buf[8..16].copy_from_slice(&7u64.to_le_bytes()); // start_vcn
        buf[0x10..0x18].copy_from_slice(&(99u64 | (1u64 << 48)).to_le_bytes());
        let refs = parse_attr_list_refs(&buf);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], (ATTR_DATA, 99, 7));
    }

    #[test]
    fn unix_to_string_known_epoch() {
        assert_eq!(unix_to_string(1_700_000_000).as_deref(), Some("2023-11-14 22:13:20"));
        assert_eq!(unix_to_string(0), None);
    }

    #[test]
    fn parse_record_rejects_non_file_magic() {
        assert!(parse_record(&vec![0u8; 1024]).is_none());
    }

    // 真实卷测试：需管理员权限 + NTFS 卷
    // 管理员 PowerShell 中运行：
    //   cargo test -p rescueforge --lib ntfs_deleted -- --ignored --nocapture
    #[test]
    #[cfg(windows)]
    #[ignore = "需要管理员权限"]
    fn real_scan_and_recover_deleted_on_d() {
        let mut counter = 0u64;
        let token = CancellationToken::new();
        let mut batch_total = 0usize;
        let outcome = scan_volume_deleted('D', None, &mut counter, &token, &mut |ev| match ev {
            DeletedScanEvent::Batch(b) => { batch_total += b.len(); }
            DeletedScanEvent::Progress(_, m) => eprintln!("  {}", m),
        });
        if let Some(note) = outcome.note {
            eprintln!("跳过: {}", note);
            return;
        }
        assert_eq!(batch_total, outcome.files.len());
        eprintln!("卷 D: 发现 {} 个已删除文件", outcome.files.len());
        for f in outcome.files.iter().take(20) {
            eprintln!("  [{}] {} ({}B) mtime={:?} -> {}", f.file_type, f.filename, f.size_bytes, f.modified_time, f.filepath);
        }
        if let Some(f) = outcome.files.iter().find(|f| f.size_bytes > 0 && f.size_bytes < 10 * 1024 * 1024) {
            let out = std::env::temp_dir().join("rf_deleted_recover_test");
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).unwrap();
            match recover_deleted_file(&f.filepath, &out, None) {
                Ok(p) => {
                    let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                    eprintln!("  [恢复成功] {} -> {} ({}B)", f.filename, p.display(), size);
                    assert!(size > 0);
                }
                Err(e) => eprintln!("  [恢复失败] {}: {}", f.filename, e),
            }
            let _ = std::fs::remove_dir_all(&out);
        }
    }
}
