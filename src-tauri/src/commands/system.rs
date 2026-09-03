use serde::Serialize;
use tauri::Manager;

/// 当前进程权限状态
#[derive(Debug, Clone, Serialize)]
pub struct PrivilegeStatus {
    /// 是否以管理员（提升）权限运行
    pub elevated: bool,
}

/// 查询当前是否管理员运行（深度恢复已删除文件、分区修复、磁盘镜像均需要管理员权限）
#[tauri::command]
pub async fn get_privilege_status() -> Result<PrivilegeStatus, String> {
    let elevated = crate::privilege::is_elevated();
    crate::app_log!("INFO", "权限状态查询: elevated={}", elevated);
    Ok(PrivilegeStatus { elevated })
}

/// 以管理员身份重启本应用（触发 UAC）。
/// 用于永久删除文件恢复（NTFS MFT 卷直读）与分区修复等需要卷/磁盘设备访问的功能。
#[tauri::command]
pub async fn restart_elevated<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    if crate::privilege::is_elevated() {
        crate::app_log!("INFO", "restart_elevated: 已是管理员权限，无需重启");
        return Err("当前已是管理员权限，无需重启".into());
    }
    // DEV 模式构建（cargo tauri dev 编译）的 exe 内嵌 devUrl（http://localhost:1420），
    // 提权重启的新实例仍会去连 dev server；若此时 dev server 已停止，新实例会直接报
    // ERR_CONNECTION_REFUSED 白屏。因此提前探测，不可达时拒绝重启并给出明确提示。
    if cfg!(dev) {
        let reachable = ["127.0.0.1:1420", "[::1]:1420"].iter().any(|addr| {
            addr.parse::<std::net::SocketAddr>()
                .ok()
                .and_then(|sa| {
                    std::net::TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(500)).ok()
                })
                .is_some()
        });
        if !reachable {
            crate::app_log!("ERROR", "restart_elevated: DEV 模式构建且 dev server(localhost:1420) 不可达，拒绝提权重启");
            return Err("当前为开发模式构建，且前端开发服务器(localhost:1420)未运行，提权重启后页面将无法加载。请使用正式构建（cargo build）的程序，或先启动开发服务器。".into());
        }
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("无法获取当前程序路径: {}", e))?;
    let exe_str = exe.to_string_lossy().to_string();
    crate::app_log!("INFO", "restart_elevated: 请求以管理员身份重启 {}", exe_str);

    #[cfg(target_os = "windows")]
    {
        // 通过 ShellExecute runas 触发 UAC；-WindowStyle Hidden 避免黑框闪现。
        // --rf-takeover：告知新实例这是提权接管，需等待旧实例退出而非被单实例守卫拦截。
        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile", "-WindowStyle", "Hidden", "-Command",
                &format!("Start-Process -FilePath '{}' -ArgumentList '--rf-takeover' -Verb RunAs", exe_str),
            ])
            .status()
            .map_err(|e| format!("启动提权进程失败: {}", e))?;
        if !status.success() {
            return Err("提权失败（可能已在 UAC 对话框中取消）".into());
        }
        crate::app_log!("INFO", "restart_elevated: 提权进程已启动，当前实例即将退出");
        // 先关闭所有 WebView 窗口，让 WebView2 子进程（msedgewebview2.exe）及时释放
        // Chromium profile 锁；否则新实例（提权）启动时 profile 仍被旧子进程锁住，
        // 导致新实例前端资源（CSS/懒加载 chunk）加载失败——表现为菜单只有图标、页面空白。
        for w in app.webview_windows().values() {
            let _ = w.close();
        }
        // 给 WebView2 子进程与 UAC 对话框一点时间，然后退出当前实例
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1200));
            app.exit(0);
        });
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("当前平台请手动以管理员权限运行本程序".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privilege_status_reflects_is_elevated() {
        // 命令返回结构必须如实反映 privilege::is_elevated（无论当前是否管理员）
        let st = PrivilegeStatus { elevated: crate::privilege::is_elevated() };
        assert_eq!(st.elevated, crate::privilege::is_elevated());
    }

    #[test]
    fn is_elevated_is_deterministic() {
        // 同一进程内多次检测结果必须一致（排除 WinAPI 调用随机失败）
        let a = crate::privilege::is_elevated();
        let b = crate::privilege::is_elevated();
        assert_eq!(a, b);
    }
}
