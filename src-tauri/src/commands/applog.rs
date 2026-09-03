//! 应用运行日志读取命令（供前端排障面板展示）。

/// 读取后端运行日志（尾部 `tail_lines` 行），返回日志文件路径与内容行。
#[tauri::command]
pub async fn get_app_log(tail_lines: Option<usize>) -> Result<serde_json::Value, String> {
    let path = crate::log::log_path();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let n = tail_lines.unwrap_or(200).max(10);
    let lines: Vec<&str> = content.lines().collect();
    let tail: Vec<&str> = lines.iter().rev().take(n).cloned().collect();
    Ok(serde_json::json!({
        "path": path.to_string_lossy(),
        "lines": tail.into_iter().rev().collect::<Vec<_>>(),
    }))
}

/// 前端写入后端运行日志（便于全链路排障）。
#[tauri::command]
pub fn write_app_log(level: String, message: String) -> Result<(), String> {
    crate::log::write_log(&level, &format!("[UI] {}", message));
    Ok(())
}

/// 自动冒烟测试开关：环境变量 RF_AUTOTEST=1 时返回 true。
#[tauri::command]
pub fn get_autotest_flag() -> bool {
    std::env::var("RF_AUTOTEST").map(|v| v == "1").unwrap_or(false)
}
