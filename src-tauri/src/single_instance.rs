//! Windows 单实例守卫（命名互斥体实现）。
//!
//! 背景：提权实例与普通实例若同时运行，会共享同一个 WebView2 用户数据目录
//! （%LOCALAPPDATA%\com.rescueforge.app\EBWebView）。Chromium 的 profile
//! 单例锁 + Windows 完整性级别（UIPI）隔离会导致后启动实例的 WebView2
//! 创建失败——窗口出现但前端完全不挂载（白屏、点击无反应）。
//!
//! 因此这里用会话命名空间内的命名互斥体做单实例控制（互斥体句柄可跨
//! 完整性级别打开，提权/非提权都能感知对方）：
//! - 已有实例在运行：激活已有主窗口，随后本进程直接退出，绝不创建 WebView。
//! - 提权重启（--rf-takeover）：等待旧实例释放互斥体（旧实例约 400ms 后退出），
//!   而不是把自己当成“第二个实例”直接退出。
//!
//! 所有失败分支都选择“继续启动”，绝不让守卫逻辑阻塞应用启动。

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, BOOL, CloseHandle, HWND, LPARAM,
};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, GetWindowTextW, IsIconic, SetForegroundWindow,
    ShowWindow, SW_RESTORE, SW_SHOWNORMAL,
};

/// 会话命名空间内的互斥体名（不加 Global\ 前缀，避免需要 SeCreateGlobalPrivilege）。
const MUTEX_NAME: &str = "RescueForge_SingleInstance_9F3A4C7E";

/// 提权重启时传给新实例的参数：允许等待旧实例退出并接管。
pub const TAKEOVER_ARG: &str = "--rf-takeover";

static FOUND_HWND: AtomicIsize = AtomicIsize::new(0);

/// 获取单实例许可。
/// 返回 true：本进程是首个实例，继续启动（互斥体句柄故意泄漏，进程退出自动释放）。
/// 返回 false：已有实例在运行，已尝试激活其窗口，本进程应退出。
pub fn acquire(takeover: bool) -> bool {
    match try_acquire() {
        Ok(already_exists) => {
            if !already_exists {
                return true;
            }
            if !takeover {
                rescueforge_lib::log::write_log(
                    "INFO",
                    "单实例守卫: 检测到已有实例在运行，激活已有窗口并退出",
                );
                unsafe { activate_existing_window() };
                return false;
            }
            // 提权接管：等待旧实例释放互斥体（旧实例调用 restart_elevated 后 ~400ms 退出）
            rescueforge_lib::log::write_log(
                "INFO",
                "单实例守卫: 提权接管模式，等待旧实例退出...",
            );
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(250));
                match try_acquire() {
                    Ok(false) => {
                        rescueforge_lib::log::write_log(
                            "INFO",
                            "单实例守卫: 旧实例已退出，接管成功",
                        );
                        return true;
                    }
                    Ok(true) => continue,
                    Err(_) => return true, // 创建失败则直接继续启动
                }
            }
            // 超时仍未释放（旧实例可能卡死）：优先保证用户能用上应用
            rescueforge_lib::log::write_log(
                "WARN",
                "单实例守卫: 等待旧实例退出超时(10s)，仍继续启动",
            );
            true
        }
        Err(_) => true, // 互斥体创建失败（极端情况）：不阻塞启动
    }
}

/// 尝试创建/打开命名互斥体。
/// Ok(true) = 互斥体已存在（另一实例在运行）；Ok(false) = 本进程独占。
fn try_acquire() -> Result<bool, ()> {
    let mut wide: Vec<u16> = MUTEX_NAME.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let handle = CreateMutexW(None, BOOL(0), PCWSTR(wide.as_ptr()))
            .map_err(|_| ())?;
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        if already {
            let _ = CloseHandle(handle);
        }
        // 独占时故意不关闭：用原始指针持有（HANDLE 是 Copy 类型，forget 无效，
        // 一旦 drop 会关闭句柄导致互斥体提前释放）。进程退出时系统自动回收。
        let _raw = handle.0;
        Ok(already)
    }
}

/// 找到已有实例的主窗口并前置激活。
unsafe fn activate_existing_window() {
    FOUND_HWND.store(0, Ordering::SeqCst);
    let current_pid = GetCurrentProcessId();
    // GetWindowTextW 返回复制的字符数（不含结尾 0），缓冲区给足即可
    let mut title_buf = [0u16; 512];
    let _ = EnumWindows(
        Some(enum_windows_cb),
        LPARAM(&mut EnumCtx {
            current_pid,
            title_buf: title_buf.as_mut_ptr(),
            title_len: title_buf.len() as i32,
        } as *mut EnumCtx as isize),
    );
    let raw = FOUND_HWND.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut _);
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    } else {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
    }
    let _ = SetForegroundWindow(hwnd);
}

struct EnumCtx {
    current_pid: u32,
    title_buf: *mut u16,
    title_len: i32,
}

unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    let mut win_pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut win_pid));
    if win_pid != ctx.current_pid {
        let n = GetWindowTextW(hwnd, std::slice::from_raw_parts_mut(
            ctx.title_buf,
            ctx.title_len as usize,
        ));
        if n >= 9 {
            let title = String::from_utf16_lossy(std::slice::from_raw_parts(ctx.title_buf, n as usize));
            if title.starts_with("RescueForge") {
                FOUND_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
                return BOOL(0); // 停止枚举
            }
        }
    }
    BOOL(1)
}
