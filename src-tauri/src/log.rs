//! 简易文件日志：追加写入 %APPDATA%\RescueForge\rescueforge.log（其他平台为用户数据目录）。
//! 无第三方依赖；写失败静默忽略，绝不影响主流程。
//! 日志文件超过 2MB 时截断保留后半部分，避免无限膨胀。

use std::io::Write;
use std::sync::Mutex;

static LOG_LOCK: Mutex<()> = Mutex::new(());

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// 日志文件路径：%APPDATA%\RescueForge\rescueforge.log
pub fn log_path() -> std::path::PathBuf {
    let base = std::env::var("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("RescueForge").join("rescueforge.log")
}

/// 追加一条日志：`[2026-08-29 12:00:00.123] [LEVEL] message`
pub fn write_log(level: &str, message: &str) {
    let _guard = match LOG_LOCK.lock() {
        Ok(g) => g,
        Err(_) => return,
    };

    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    // 超限则截断保留后半部分
    if let Ok(md) = std::fs::metadata(&path) {
        if md.len() > MAX_LOG_BYTES {
            if let Ok(data) = std::fs::read(&path) {
                let keep_from = (data.len() / 2) as usize;
                let _ = std::fs::write(&path, &data[keep_from..]);
            }
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // 简易 UTC+8 本地时间格式化（避免引入 chrono）
    let local = secs + 8 * 3600;
    let days = local / 86400;
    let sod = local % 86400;
    let (h, m, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    // 从 1970-01-01 推算日期
    let (y, mo, d) = civil_from_days(days as i64);

    let line = format!(
        "[{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}] [{}] {}\n",
        y, mo, d, h, m, s,
        now.subsec_millis(), level, message
    );

    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
    // 同时输出到控制台，方便 dev 模式观察
    eprint!("{}", line);
}

/// Howard Hinnant 的 civil_from_days 算法：自 1970-01-01 的天数 -> (年, 月, 日)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

#[macro_export]
macro_rules! app_log {
    ($level:expr, $($arg:tt)*) => {
        $crate::log::write_log($level, &format!($($arg)*))
    };
}
