use tauri::{AppHandle, Emitter, State};
use std::sync::Arc;
use crate::scheduler::{TaskScheduler, ScanJob, JobPriority, CancellationToken};
use crate::ScanEventPayload;
use serde::Serialize;

pub struct SchedulerState {
    pub scheduler: Arc<std::sync::Mutex<TaskScheduler>>,
    pub active_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>>,
    /// 会话完整结果（后端权威副本）。深度扫描可发现数万文件，
    /// 绝不能一次性推给前端（会卡死 JS 主线程），前端通过 get_scan_results 分页拉取。
    pub session_results: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<FoundFile>>>>,
}

/// 单个事件最多内联携带的结果文件数（预览用）。
/// 超过该数量时完整结果由 get_scan_results 分页获取，避免巨型 IPC 消息卡死前端。
const EVENT_RESULT_CAP: usize = 2000;

/// 扫描发现的文件记录。
/// `deleted == true` 表示深度恢复阶段发现的「已删除/已剪走」文件（回收站或 NTFS MFT 残留）。
#[derive(Debug, Clone, Serialize)]
pub struct FoundFile {
    pub id: String,
    pub filename: String,
    pub filepath: String,
    pub extension: String,
    pub size_bytes: u64,
    pub file_type: String,
    pub modified_time: Option<String>,
    #[serde(default)]
    pub deleted: bool,
    /// 删除前的完整原始路径（如 `D:\temp\sub\a.txt`），仅 MFT 深度恢复来源有值。
    /// 同名文件被多次删除时，靠此字段区分它们各自原本所在的目录。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    /// 数据区是否仍完好：`Some(false)` = 数据簇疑似已被覆写（读到全零），
    /// 恢复出来多半是空文件/打不开。扫描阶段即检测，让用户提前知道哪些救不回来。
    #[serde(default)]
    pub data_intact: Option<bool>,
}

impl FoundFile {
    /// 构造非删除来源（现存文件/回收站）的记录
    pub fn new_found(
        id: String, filename: String, filepath: String, extension: String,
        size_bytes: u64, file_type: String, modified_time: Option<String>, deleted: bool,
    ) -> Self {
        Self { id, filename, filepath, extension, size_bytes, file_type, modified_time, deleted, original_path: None, data_intact: None }
    }
}

// ===== Tauri 命令 =====

#[tauri::command]
pub async fn select_folder() -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // 必须先强制 stdout 为 UTF-8，否则 PowerShell 默认用控制台代码页（GBK）输出，
        // 中文路径（如 D:\新建文件夹）会被 Rust 端按 UTF-8 解码成乱码。
        let ps_script = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = "选择文件夹"
$dialog.ShowNewFolderButton = $true
if ($dialog.ShowDialog() -eq 'OK') { Write-Output $dialog.SelectedPath }
"#;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-Command", ps_script])
            .output().map_err(|e| format!("{}", e))?;
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if path.is_empty() { None } else { Some(path) })
    }
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("osascript")
            .args(["-e", r#"POSIX path of (choose folder)"#])
            .output().map_err(|e| format!("{}", e))?;
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if path.is_empty() { None } else { Some(path) })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::process::Command;
        let output = Command::new("zenity")
            .args(["--file-selection", "--directory", "--title=选择文件夹"])
            .output().map_err(|_| "请安装 zenity".to_string())?;
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(if path.is_empty() { None } else { Some(path) })
    }
}

#[tauri::command]
pub async fn start_path_scan<R: tauri::Runtime>(
    app_handle: AppHandle<R>,
    state: State<'_, SchedulerState>,
    paths: Vec<String>,
    mode: String,
    signature_ids: Option<Vec<String>>,
) -> Result<String, String> {
    crate::app_log!("INFO", "start_path_scan 被调用: paths={:?} mode={}", paths, mode);
    if paths.is_empty() { return Err("至少需要一个扫描路径".into()); }

    let (valid_paths, invalid_paths) = prepare_scan_paths(paths);
    if valid_paths.is_empty() {
        crate::app_log!("WARN", "无有效扫描路径: invalid={:?}", invalid_paths);
        return Err(format!("路径不存在或不可访问: {}", invalid_paths.join(", ")));
    }
    let skipped_note = if invalid_paths.is_empty() {
        String::new()
    } else {
        format!("（已跳过 {} 个无效路径: {}）", invalid_paths.len(), invalid_paths.join(", "))
    };
    let paths = valid_paths;

    let session_id = format!("ps-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());

    let token = CancellationToken::new();
    {
        let mut tokens = state.active_tokens.lock().unwrap();
        tokens.insert(session_id.clone(), token.clone());
    }

    let job = ScanJob {
        id: session_id.clone(), disk_index: 0, scan_mode: mode.clone(),
        target_paths: Some(paths.clone()),
        signature_ids: Some(signature_ids.unwrap_or_default()),
        priority: JobPriority::Normal, cancel_token: token,
    };
    { state.scheduler.lock().unwrap().submit(job); }

    // Fire first event IMMEDIATELY so UI shows something
    let _ = app_handle.emit("scan-event", ScanEventPayload {
        status: "scanning".into(), progress: 0.0,
        message: format!("开始扫描 {} 个路径{}", paths.len(), skipped_note),
        found_files: Some(0), speed_mbps: None,
        session_id: Some(session_id.clone()),
        current_path: Some(paths[0].clone()), path_progress: Some(0.0),
        result_files: Some(vec![]),
    });

    let app = app_handle.clone();
    let sid = session_id.clone();
    let tokens = state.active_tokens.clone();
    let results = state.session_results.clone();
    // 用独立 OS 线程跑扫描：Tauri async 命令上下文没有 tokio reactor，
    // tokio::spawn 在此失效（任务不会被调度），改用 std::thread。
    crate::app_log!("INFO", "扫描会话 {} 已创建，首事件已 emit，启动扫描线程", session_id);
    std::thread::spawn(move || {
        crate::app_log!("INFO", "扫描线程已启动, session={}", sid);
        scan_folders_sync(app, sid, paths, tokens, results);
        crate::app_log!("INFO", "扫描线程退出");
    });

    Ok(session_id)
}

#[tauri::command]
pub async fn start_scan<R: tauri::Runtime>(
    app_handle: AppHandle<R>, state: State<'_, SchedulerState>,
    disk_index: u32, mode: String, signature_ids: Option<Vec<String>>,
) -> Result<String, String> {
    crate::app_log!("INFO", "start_scan 被调用: disk_index={} mode={}", disk_index, mode);
    // 磁盘索引映射到盘符（枚举顺序与 get_disks 一致），整盘扫描 = 盘根目录遍历 + 深度恢复阶段（回收站 + MFT 已删除文件）
    let drive = crate::commands::disk::drive_letter_for_index(disk_index)
        .ok_or_else(|| format!("找不到磁盘 {}（请在仪表盘刷新磁盘列表）", disk_index))?;
    let root = format!("{}:\\", drive);
    crate::app_log!("INFO", "磁盘扫描: 磁盘 {} -> 根路径 {}", disk_index, root);

    let session_id = format!("scan-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
    let token = CancellationToken::new();
    { let mut tokens = state.active_tokens.lock().unwrap(); tokens.insert(session_id.clone(), token.clone()); }
    let job = ScanJob {
        id: session_id.clone(), disk_index, scan_mode: mode.clone(),
        target_paths: Some(vec![root.clone()]), signature_ids: Some(signature_ids.unwrap_or_default()),
        priority: JobPriority::Normal, cancel_token: token,
    };
    { state.scheduler.lock().unwrap().submit(job); }

    let _ = app_handle.emit("scan-event", ScanEventPayload {
        status: "scanning".into(), progress: 0.0,
        message: format!("开始磁盘扫描: {} 盘（现存文件 + 回收站 + 已删除文件）", drive),
        found_files: Some(0), speed_mbps: None,
        session_id: Some(session_id.clone()),
        current_path: Some(root.clone()), path_progress: Some(0.0), result_files: Some(vec![]),
    });

    let app = app_handle.clone();
    let sid = session_id.clone();
    let tokens = state.active_tokens.clone();
    let results = state.session_results.clone();
    std::thread::spawn(move || {
        crate::app_log!("INFO", "磁盘扫描线程已启动, session={}", sid);
        scan_folders_sync(app, sid, vec![root], tokens, results);
        crate::app_log!("INFO", "磁盘扫描线程退出");
    });
    Ok(session_id)
}

#[tauri::command]
pub async fn pause_scan<R: tauri::Runtime>(app_handle: AppHandle<R>, state: State<'_, SchedulerState>, session_id: String) -> Result<(), String> {
    { let tokens = state.active_tokens.lock().unwrap(); if let Some(t) = tokens.get(&session_id) { t.pause(); } }
    let _ = app_handle.emit("scan-event", ScanEventPayload {
        status: "paused".into(), progress: 0.0, message: "已暂停".into(),
        found_files: None, speed_mbps: None, session_id: Some(session_id),
        current_path: None, path_progress: None, result_files: None,
    });
    Ok(())
}

#[tauri::command]
pub async fn resume_scan<R: tauri::Runtime>(app_handle: AppHandle<R>, state: State<'_, SchedulerState>, session_id: String) -> Result<(), String> {
    { let tokens = state.active_tokens.lock().unwrap(); if let Some(t) = tokens.get(&session_id) { t.resume(); } }
    let _ = app_handle.emit("scan-event", ScanEventPayload {
        status: "scanning".into(), progress: 0.0, message: "已继续".into(),
        found_files: None, speed_mbps: None, session_id: Some(session_id),
        current_path: None, path_progress: None, result_files: None,
    });
    Ok(())
}

#[tauri::command]
pub async fn cancel_scan<R: tauri::Runtime>(app_handle: AppHandle<R>, state: State<'_, SchedulerState>, session_id: String) -> Result<(), String> {
    { let tokens = state.active_tokens.lock().unwrap(); if let Some(t) = tokens.get(&session_id) { t.cancel(); } }
    let _ = app_handle.emit("scan-event", ScanEventPayload {
        status: "cancelled".into(), progress: 0.0, message: "已取消".into(),
        found_files: None, speed_mbps: None, session_id: Some(session_id),
        current_path: None, path_progress: None, result_files: None,
    });
    Ok(())
}

/// 在系统文件管理器中打开目录（恢复完成后引导用户查看输出）
#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { std::process::Command::new("explorer").arg(&path).spawn().map_err(|e| e.to_string())?; }
    #[cfg(target_os = "macos")]
    { std::process::Command::new("open").arg(&path).spawn().map_err(|e| e.to_string())?; }
    #[cfg(all(unix, not(target_os = "macos")))]
    { std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?; }
    Ok(())
}

/// 路径校验结果（供前端手动输入路径时实时校验）
#[derive(Debug, Clone, Serialize)]
pub struct PathValidation {
    pub normalized: String,
    pub kind: String, // "dir" | "file" | "missing"
}

#[tauri::command]
pub async fn validate_path(path: String) -> Result<PathValidation, String> {
    let normalized = normalize_path_input(&path);
    if normalized.is_empty() {
        return Err("路径不能为空".into());
    }
    let kind = match classify_path(&normalized) {
        PathKind::Dir => "dir",
        PathKind::File => "file",
        PathKind::Missing => "missing",
    };
    Ok(PathValidation { normalized, kind: kind.into() })
}

#[tauri::command]
pub async fn recover_files<R: tauri::Runtime>(
    app_handle: AppHandle<R>, file_ids: Vec<String>, output_path: String,
) -> Result<serde_json::Value, String> {
    let mut emit = |payload: ScanEventPayload| { let _ = app_handle.emit("scan-event", payload); };
    crate::app_log!("INFO", "recover_files 被调用: {} 个文件 -> {}", file_ids.len(), output_path);
    let r = recover_files_core(&file_ids, &output_path, &mut emit);
    match &r {
        Ok(v) => crate::app_log!("INFO", "recover_files 完成: {}", v),
        Err(e) => crate::app_log!("ERROR", "recover_files 失败: {}", e),
    }
    r
}

/// 恢复核心逻辑（与 AppHandle 解耦）：解析前端传入的 "id||真实路径"，
/// 逐个复制到输出目录，并通过注入的 `emit` 回调推送逐文件进度。
/// 返回 { success, failed, output_path }。
pub fn recover_files_core(
    file_ids: &[String], output_path: &str,
    emit: &mut dyn FnMut(ScanEventPayload),
) -> Result<serde_json::Value, String> {
    std::fs::create_dir_all(output_path).map_err(|e| format!("{}", e))?;
    let total = file_ids.len();
    let output_dir = std::path::Path::new(output_path);
    let mut ok = 0u64; let mut fail = 0u64;

    for (i, fid) in file_ids.iter().enumerate() {
        // file_id 格式：
        //   "id||ntfs://{盘符}:/{记录号}[||original_path]"           -> MFT 卷直读恢复
        //   "id||recycleresidue://{盘符}:/{记录号}[||original_path]"  -> 回收站清空残留 $R
        //   "id||recycle://{R路径}||{原始文件名}"                    -> 复制回收站 $R（内含自身 || 分隔）
        // 注意：recycle:// 的编码自带 || 分隔原始文件名，需先按前缀区分再拆分。
        let body = fid.split_once("||").map(|(_, rest)| rest).unwrap_or(fid);
        let name_hint = body.rsplit(['/', '\\']).next().unwrap_or(body).to_string();

        let result: Result<std::path::PathBuf, std::io::Error>;
        if body.starts_with(RECYCLE_PREFIX) {
            let rest = &body[RECYCLE_PREFIX.len()..];
            let (rpath, orig) = match rest.split_once("||") {
                Some((p, n)) => (p, n),
                None => (rest, ""),
            };
            result = recover_recycle_file(std::path::Path::new(rpath), orig, output_dir);
        } else if body.starts_with(super::ntfs_deleted::NTFS_PREFIX)
            || body.starts_with(super::ntfs_deleted::RECYCLE_RESIDUE_PREFIX) {
            // 第三段（若存在）是 original_path，取其文件名作为落盘名
            let (src, orig) = match body.split_once("||") {
                Some((s, o)) => (s, o),
                None => (body, ""),
            };
            let display = if orig.is_empty() {
                None
            } else {
                std::path::Path::new(orig).file_name().map(|n| n.to_string_lossy().to_string())
            };
            result = super::ntfs_deleted::recover_deleted_file(src, output_dir, display.as_deref());
        } else {
            let src_path = std::path::Path::new(body);
            let name = src_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(format!("f_{}.bin", i));
            result = recover_one_file(src_path, output_dir)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {}", name, e)));
        }
        let name = name_hint;
        match result {
            Ok(dest) => { ok += 1; emit(ScanEventPayload {
                status: "recovering".into(), progress: (i+1) as f32 / total as f32,
                message: format!("已恢复: {}", dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(name)),
                found_files: None, speed_mbps: None, session_id: None,
                current_path: None, path_progress: None, result_files: None,
            });}
            Err(e) => { fail += 1; emit(ScanEventPayload {
                status: "recovering".into(), progress: (i+1) as f32 / total as f32,
                message: format!("失败 {}: {}", name, e),
                found_files: None, speed_mbps: None, session_id: None,
                current_path: None, path_progress: None, result_files: None,
            });}
        }
    }
    Ok(serde_json::json!({"success": ok, "failed": fail, "output_path": output_path}))
}

#[tauri::command]
pub async fn preview_file(filepath: String) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let f = std::fs::File::open(&filepath).map_err(|e| format!("{}", e))?;
    let mut buf = Vec::new();
    f.take(1_048_576).read_to_end(&mut buf).map_err(|e| format!("{}", e))?;
    Ok(buf)
}

#[tauri::command]
pub async fn get_file_info(filepath: String) -> Result<FoundFile, String> {
    let p = std::path::Path::new(&filepath);
    if !p.exists() { return Err("不存在".into()); }
    let size = p.metadata().map(|m| m.len()).unwrap_or(0);
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(FoundFile::new_found(
        "f-0".into(), name, filepath, ext,
        size, "Unknown".into(), None, false,
    ))
}

/// 分页获取会话的完整扫描结果（后端权威副本）。
/// 深度扫描可能发现数万文件，事件只内联前 EVENT_RESULT_CAP 条预览，
/// 前端用本命令按 offset/limit 分批拉取，避免一次性巨型消息卡死 UI。
#[tauri::command]
pub async fn get_scan_results(
    state: State<'_, SchedulerState>, session_id: String,
    offset: Option<usize>, limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(1000).min(5000);
    let map = state.session_results.lock().unwrap();
    let empty: Vec<FoundFile> = Vec::new();
    let files = map.get(&session_id).unwrap_or(&empty);
    let total = files.len();
    let slice: Vec<FoundFile> = files.iter().skip(offset).take(limit).cloned().collect();
    Ok(serde_json::json!({ "total": total, "offset": offset, "files": slice }))
}

// ===== 路径处理（纯函数，可单测） =====

/// 路径类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    Dir,
    File,
    Missing,
}

/// 规范化用户手动输入的路径：去首尾空白、去成对引号、Windows 下统一分隔符
pub fn normalize_path_input(input: &str) -> String {
    let trimmed = input.trim();
    let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    #[cfg(windows)]
    {
        unquoted.replace('/', "\\")
    }
    #[cfg(not(windows))]
    {
        unquoted.to_string()
    }
}

/// 判断路径是目录、文件还是不存在
pub fn classify_path(path: &str) -> PathKind {
    match std::fs::metadata(path) {
        Ok(md) if md.is_dir() => PathKind::Dir,
        Ok(md) if md.is_file() => PathKind::File,
        _ => PathKind::Missing,
    }
}

/// 规范化并校验一批扫描路径，返回 (有效路径, 无效路径)，有效路径已去重
pub fn prepare_scan_paths(inputs: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut valid: Vec<String> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    for raw in inputs {
        let normalized = normalize_path_input(&raw);
        if normalized.is_empty() { continue; }
        match classify_path(&normalized) {
            PathKind::Missing => invalid.push(normalized),
            _ => {
                if !valid.contains(&normalized) {
                    valid.push(normalized);
                }
            }
        }
    }
    (valid, invalid)
}

// ===== 文件恢复（纯函数，可单测） =====

/// 回收站数据文件编码前缀：recycle://{R文件完整路径}||{原始文件名}
pub const RECYCLE_PREFIX: &str = "recycle://";

/// 在输出目录中为文件名找一个不冲突的落盘路径（重名自动加 " (n)"，绝不覆盖）
pub fn unique_dest_path(output_dir: &std::path::Path, fname: &str) -> std::io::Result<std::path::PathBuf> {
    let name = if fname.is_empty() { "recovered.bin".to_string() } else { fname.to_string() };
    let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| name.clone());
    let ext = std::path::Path::new(&name).extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let mut dest = output_dir.join(&name);
    let mut n = 1u32;
    while dest.exists() {
        let candidate = if ext.is_empty() {
            format!("{} ({})", stem, n)
        } else {
            format!("{} ({}).{}", stem, n, ext)
        };
        dest = output_dir.join(candidate);
        n += 1;
        if n > 9999 {
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "重名冲突过多"));
        }
    }
    Ok(dest)
}

/// 复制单个文件到输出目录；重名时自动追加 " (n)" 后缀，绝不覆盖已有文件
pub fn recover_one_file(src: &std::path::Path, output_dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    if !src.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("源文件不存在: {}", src.display()),
        ));
    }
    std::fs::create_dir_all(output_dir)?;
    let name = src.file_name().map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "recovered.bin".to_string());
    let dest = unique_dest_path(output_dir, &name)?;
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

/// 恢复回收站数据文件：复制 $R 数据文件，并按 $I 元数据中的原始文件名落盘。
/// 原始文件名的非法字符会被替换，避免写入失败。
pub fn recover_recycle_file(src: &std::path::Path, orig_name: &str, output_dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    if !src.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("回收站数据文件不存在（可能已被清空）: {}", src.display()),
        ));
    }
    std::fs::create_dir_all(output_dir)?;
    let fallback = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "recovered.bin".to_string());
    let raw = if orig_name.trim().is_empty() { fallback } else { orig_name.to_string() };
    let clean: String = raw.chars().map(|c| match c {
        '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
        '/' | '\\' => '_',
        _ => c,
    }).collect();
    let dest = unique_dest_path(output_dir, &clean)?;
    std::fs::copy(src, &dest)?;
    Ok(dest)
}

// ===== 文件收集引擎（纯函数，可单测） =====

/// 收集任意路径（目录或单文件）下的所有文件。
/// `on_progress(total, batch)` 按节流间隔（~200ms）实时回调；每个子目录遍历结束时强制推送一次，
/// 保证即使只有几个文件，前端也能收到进度事件（修复小目录扫描进度卡 0 的问题）。
/// `control` 提供协作式取消/暂停：每个条目检查一次，取消则提前返回已收集部分。
/// 进度推送节流间隔：最多每 200ms 推送一次，避免海量小文件场景下事件风暴；
/// 每个目录遍历结束时会强制刷新一次，保证小目录也能实时反馈进度。
const PROGRESS_FLUSH_MS: std::time::Duration = std::time::Duration::from_millis(200);

/// 收集引擎内部状态：待推送缓冲 + 节流时间戳 + 计数器，实现细粒度实时进度反馈。
struct CollectState<'a> {
    counter: &'a mut u64,
    control: &'a CancellationToken,
    pending: Vec<FoundFile>,
    last_flush: std::time::Instant,
    on_progress: &'a mut dyn FnMut(u64, &[FoundFile]),
}

impl<'a> CollectState<'a> {
    /// 将缓冲中的文件推送给上层；`force` 为 false 时受节流间隔约束。空缓冲不推送。
    fn flush(&mut self, force: bool) {
        if self.pending.is_empty() { return; }
        if !force && self.last_flush.elapsed() < PROGRESS_FLUSH_MS { return; }
        (self.on_progress)(*self.counter, &self.pending);
        self.pending.clear();
        self.last_flush = std::time::Instant::now();
    }
}

pub fn collect_files(
    path: &std::path::Path,
    depth: u32,
    counter: &mut u64,
    control: &CancellationToken,
    on_progress: &mut dyn FnMut(u64, &[FoundFile]),
) -> Vec<FoundFile> {
    let mut out: Vec<FoundFile> = Vec::new();
    let mut st = CollectState {
        counter,
        control,
        pending: Vec::new(),
        // 起点回拨一个节流窗口，首个文件立即可推，进度第一时间动起来
        last_flush: std::time::Instant::now() - PROGRESS_FLUSH_MS,
        on_progress,
    };
    collect_inner(path, depth, &mut st, &mut out);
    // 收尾：确保剩余缓冲全部推送（例如不足节流窗口的最后一批）
    st.flush(true);
    out
}

fn collect_inner(
    path: &std::path::Path,
    depth: u32,
    st: &mut CollectState,
    out: &mut Vec<FoundFile>,
) {
    if depth > 15 { return; }
    // 协作式取消/暂停检查点
    if !st.control.wait_while_paused() { st.flush(true); return; }

    match classify_path(&path.to_string_lossy()) {
        PathKind::File => {
            if let Some(f) = make_found_file(path, st.counter) {
                st.pending.push(f.clone());
                out.push(f);
                st.flush(false);
            }
        }
        PathKind::Dir => {
            let entries = match std::fs::read_dir(path) {
                Ok(e) => e,
                Err(_) => return,
            };
            for entry in entries.flatten() {
                // 每个条目做协作式取消/暂停检查（原子读，开销可忽略）
                if !st.control.wait_while_paused() { st.flush(true); return; }
                let entry_path = entry.path();
                let ft = match entry.file_type() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if ft.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('$') || name == "System Volume Information" || name == "node_modules" {
                        continue;
                    }
                    collect_inner(&entry_path, depth + 1, st, out);
                } else if ft.is_file() {
                    if let Some(f) = make_found_file(&entry_path, st.counter) {
                        st.pending.push(f.clone());
                        out.push(f);
                        st.flush(false);
                    }
                }
            }
            // 目录遍历结束：即使不足节流窗口也强制推送一次，
            // 保证小目录（不足 100 文件）场景下进度不会卡在 0%。
            st.flush(true);
        }
        PathKind::Missing => {}
    }
}

/// 由文件路径构造 FoundFile；空文件或不可读文件返回 None
fn make_found_file(path: &std::path::Path, counter: &mut u64) -> Option<FoundFile> {
    let size = path.metadata().ok()?.len();
    if size == 0 { return None; }
    *counter += 1;
    Some(FoundFile::new_found(
        format!("f-{}", *counter),
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        path.to_string_lossy().to_string(),
        path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
        size,
        quick_detect(path).to_string(),
        None,
        false,
    ))
}

// ===== 深度恢复：回收站扫描（纯函数 + 目录遍历，可单测） =====
//
// Windows 回收站布局：{盘符}:\$RECYCLE.BIN\{用户SID}\$Ixxxx.ext + $Rxxxx.ext
// $I 为元数据（原始路径/删除时间），$R 为真实数据；普通目录扫描会跳过 $ 开头目录，
// 因此回收站内容只能由本阶段专门发现。
/// 回收站扫描单批上限，避免异常卷上元数据条目过多拖死扫描。
const RECYCLE_MAX_FILES: usize = 20_000;

/// 解析回收站 $I 元数据文件，返回 (原始完整路径, 删除时间 Unix 秒)。
/// 兼容 v1（Win7，固定 260 字符宽路径）与 v2（Win8+，带长度前缀）两种格式。
pub fn parse_recycle_info_meta(buf: &[u8]) -> Option<(String, u64)> {
    if buf.len() < 24 { return None; }
    let ver = u64::from_le_bytes(buf[0..8].try_into().ok()?);
    let ftime = u64::from_le_bytes(buf[16..24].try_into().ok()?);
    let unix = (ftime / 10_000_000).saturating_sub(11_644_473_600);
    let wide: Vec<u16> = if ver == 2 && buf.len() >= 28 {
        // $I v2（Win8+）布局：0x18 起 **4 字节** 为路径字符数，0x1C(28) 起才是 UTF-16LE 路径。
        // 曾按「2 字节长度 + 从 26 开始」解析，少偏移 2 字节，把长度字段高 2 字节（值 0）
        // 当成路径首字符，得到 "\0D:\temp\xxx"，导致后续按目录过滤时全部误杀。
        let n = u32::from_le_bytes(buf[24..28].try_into().ok()?) as usize;
        if n == 0 || buf.len() < 28 + n * 2 { return None; }
        let mut w = Vec::with_capacity(n);
        for i in 0..n {
            w.push(u16::from_le_bytes(buf[28 + i * 2..30 + i * 2].try_into().ok()?));
        }
        w
    } else if buf.len() >= 24 + 520 {
        // v1: 偏移 24 起固定 260 个 UTF-16 字符（总 520 字节），空字符结尾
        let mut w = Vec::new();
        for i in 0..260 {
            let c = u16::from_le_bytes(buf[24 + i * 2..26 + i * 2].try_into().ok()?);
            if c == 0 { break; }
            w.push(c);
        }
        w
    } else {
        return None;
    };
    // 字符数可能把结尾的 null 终止符也算进去，统一裁掉首尾空字符，
    // 否则路径会带前导/尾随 \0，导致按目录过滤时误判。
    let path = String::from_utf16_lossy(&wide).trim_matches('\0').to_string();
    if path.is_empty() { return None; }
    Some((path, unix))
}

/// 从扫描路径列表提取去重后的盘符（如 "D:\temp" -> 'D'），非绝对盘符路径忽略。
pub fn drive_letters_of(paths: &[String]) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for p in paths {
        let b = p.as_bytes();
        if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
            let c = (b[0] as char).to_ascii_uppercase();
            if !out.contains(&c) { out.push(c); }
        }
    }
    out
}

/// 扫描指定盘符的回收站，找出可恢复的已删除文件。
/// 无权限/无回收站时静默返回空结果；`on_batch` 每攒够一批回调一次。
/// `target_dir` 为 Some 时只返回原始路径位于该目录下（含子目录）的回收站文件。
pub fn scan_recycle_bin(
    drive: char,
    target_dir: Option<&str>,
    counter: &mut u64,
    token: &CancellationToken,
    on_batch: &mut dyn FnMut(&[FoundFile]),
) -> Vec<FoundFile> {
    let base = std::path::PathBuf::from(format!("{}:\\$RECYCLE.BIN", drive));
    let sid_dirs = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut files: Vec<FoundFile> = Vec::new();
    let mut pending: Vec<FoundFile> = Vec::new();
    'outer: for sid in sid_dirs.flatten() {
        if !sid.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        let entries = match std::fs::read_dir(sid.path()) { Ok(e) => e, Err(_) => continue };
        for entry in entries.flatten() {
            if !token.wait_while_paused() || token.is_cancelled() { break 'outer; }
            if files.len() + pending.len() >= RECYCLE_MAX_FILES { break 'outer; }
            let ename = entry.file_name().to_string_lossy().to_string();
            if !ename.starts_with("$I") || ename.len() < 3 { continue; }
            let meta = match std::fs::read(entry.path()) { Ok(m) => m, Err(_) => continue };
            let (orig_path, unix) = match parse_recycle_info_meta(&meta) {
                Some(x) => x,
                None => continue,
            };
            // 限定目录：回收站 $I 元数据自带删除前完整路径，直接据此过滤
            if let Some(t) = target_dir {
                if !super::ntfs_deleted::path_under_dir(&orig_path, t) { continue; }
            }
            let r_name = format!("$R{}", &ename[2..]);
            let r_path = sid.path().join(&r_name);
            if !r_path.is_file() { continue; }
            let size = match r_path.metadata() { Ok(m) => m.len(), Err(_) => continue };
            if size == 0 { continue; }
            let orig_name = std::path::Path::new(&orig_path)
                .file_name().map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| r_name.clone());
            *counter += 1;
            pending.push(FoundFile {
                id: format!("f-{}", *counter),
                filename: orig_name.clone(),
                filepath: format!("{}{}||{}", RECYCLE_PREFIX, r_path.to_string_lossy(), orig_name),
                extension: std::path::Path::new(&orig_name)
                    .extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default(),
                size_bytes: size,
                file_type: quick_detect(&r_path).to_string(),
                modified_time: super::ntfs_deleted::unix_to_string(unix),
                deleted: true,
                // 回收站 $I 元数据已记录删除前完整路径，直接展示给用户
                original_path: Some(orig_path.clone()),
                // $R 是真实存在的载体文件（不存在时上面已跳过），数据必然完好
                data_intact: Some(true),
            });
            if pending.len() >= 50 {
                let batch: Vec<FoundFile> = pending.drain(..).collect();
                files.extend(batch.iter().cloned());
                on_batch(&batch);
            }
        }
    }
    if !pending.is_empty() {
        let batch: Vec<FoundFile> = pending.drain(..).collect();
        files.extend(batch.iter().cloned());
        on_batch(&batch);
    }
    crate::app_log!("INFO", "盘符 {} 回收站扫描结束: {} 个已删除文件", drive, files.len());
    files
}

// ===== 扫描调度：逐路径收集，实时推送 =====
//
// `run_path_scan_with` 是与 AppHandle 解耦的扫描编排核心：事件通过注入的 `emit` 闭包推送。
// 生产路径由 `scan_folders_sync` 接入 Tauri emit；测试路径用普通闭包收集事件，
// 从而在无 GUI 环境下端到端验证完整事件流（进度不卡 0 / 完成事件 / 结果文件列表）。
pub fn run_path_scan_with(
    session_id: &str,
    paths: &[String],
    token: &CancellationToken,
    tokens_cleanup: &dyn Fn(),
    deep: bool,
    emit: &mut dyn FnMut(ScanEventPayload),
    save_results: &dyn Fn(Vec<FoundFile>),
) {
    let mut all_files: Vec<FoundFile> = Vec::new();
    let mut file_counter: u64 = 0;
    let sid = session_id.to_string();
    // 深度恢复阶段的降级提示（如「卷需要管理员权限」）。提到函数级，
    // 以便写进最终 completed 消息——否则用户只看完成提示会误以为已完整扫描。
    let mut notes: Vec<String> = Vec::new();

    for (pi, path) in paths.iter().enumerate() {
        // 协作式取消检查：取消时把已收集的部分结果一并推给前端（限流预览）
        if token.is_cancelled() {
            let total = all_files.len() as u64;
            let preview: Vec<FoundFile> = all_files.iter().take(EVENT_RESULT_CAP).cloned().collect();
            save_results(all_files);
            emit(ScanEventPayload {
                status: "cancelled".into(), progress: 0.0, message: "已取消".into(),
                found_files: Some(total), speed_mbps: None,
                session_id: Some(sid.clone()), current_path: Some(path.clone()),
                path_progress: None, result_files: Some(preview),
            });
            tokens_cleanup();
            return;
        }

        let dir_name = std::path::Path::new(path).file_name()
            .map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());

        // Path start event
        emit(ScanEventPayload {
            status: "scanning".into(), progress: pi as f32 / paths.len() as f32,
            message: format!("[{}/{}] 正在扫描: {}", pi+1, paths.len(), dir_name),
            found_files: Some(0), speed_mbps: None,
            session_id: Some(sid.clone()), current_path: Some(path.clone()),
            path_progress: Some(0.0), result_files: Some(vec![]),
        });

        // 同步收集（调用方已保证运行在独立 OS 线程，无需再套 spawn_blocking）
        let sid_cb = sid.clone();
        let label_cb = dir_name.clone();
        let cur_cb = path.clone();
        let path_buf = std::path::PathBuf::from(path);
        let token_cb = token.clone();
        let total_paths = paths.len();
        let files_before = file_counter;
        // 重借用：闭包只持有本轮迭代的短暂可变引用，循环体结束后释放，供后续 emit 继续使用。
        let emit_cb = &mut *emit;
        let mut on_progress = move |total: u64, batch: &[FoundFile]| {
            // 路径内进度估算：以累计文件数的对数平滑推进（无法预知总数），
            // 避免单路径扫描时整体进度长时间停在同一点。
            let within = total.saturating_sub(files_before);
            let frac = ((within as f64).ln().max(0.0) / 14.0).min(0.95);
            emit_cb(ScanEventPayload {
                status: "scanning".into(),
                progress: ((pi as f32 + frac as f32) / total_paths as f32).min(0.99),
                message: format!("[{}] 已扫描 {} 个文件", label_cb, total),
                found_files: Some(total), speed_mbps: None,
                session_id: Some(sid_cb.clone()),
                current_path: Some(cur_cb.clone()), path_progress: Some(frac as f32),
                result_files: Some(batch.iter().rev().take(50).rev().cloned().collect()),
            });
        };
        let batch = collect_files(&path_buf, 0, &mut file_counter, &token_cb, &mut on_progress);
        drop(on_progress);
        all_files.extend(batch);

        emit(ScanEventPayload {
            status: "scanning".into(), progress: (pi + 1) as f32 / paths.len() as f32,
            message: format!("[{}/{}] 完成: {} — 累计 {} 文件", pi+1, paths.len(), dir_name, all_files.len()),
            found_files: Some(all_files.len() as u64), speed_mbps: None,
            session_id: Some(sid.clone()), current_path: Some(path.clone()),
            path_progress: Some(1.0),
            result_files: Some(all_files.iter().rev().take(100).rev().cloned().collect()),
        });
    }

    // ===== 阶段2：深度恢复（回收站 + NTFS 卷级已删除文件） =====
    // 找回「已删除/已剪走」的文件：普通目录遍历看不到它们，必须专门解析回收站元数据与 MFT 残留。
    // 关键：用户扫描哪个目录，就只恢复该目录下的已删除文件（回收站按 $I 原始路径过滤，
    // MFT 按父目录引用链重建路径后过滤），避免整卷结果淹没目标目录的少量结果。
    let drives = if deep { drive_letters_of(paths) } else { Vec::new() };
    if !drives.is_empty() {
        // 目标目录列表：用户在阶段1输入的路径（可能是文件，取其所在目录）
        let target_dirs: Vec<String> = paths.iter().map(|p| {
            let pb = std::path::PathBuf::from(p);
            if pb.is_dir() { p.trim_end_matches(['\\', '/']).to_string() }
            else { pb.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| p.clone()) }
        }).collect();
        let existing_count = all_files.len();
        crate::app_log!("INFO", "进入深度恢复阶段: 卷={:?}，当前已有 {} 个现存文件", drives, existing_count);
        emit(ScanEventPayload {
            status: "scanning".into(), progress: 0.90,
            message: format!("进入深度恢复阶段: 扫描 {} 个卷的回收站与已删除文件", drives.len()),
            found_files: Some(existing_count as u64), speed_mbps: None,
            session_id: Some(sid.clone()), current_path: None, path_progress: None,
            result_files: Some(vec![]),
        });

        // 2a: 回收站（$I 元数据 + $R 数据，无需管理员）
        let mut recycle_stage: Vec<FoundFile> = Vec::new();
        {
            let emit_cb = &mut *emit;
            let stage_cb = &mut recycle_stage;
            let sid_cb = sid.clone();
            let base_count = existing_count;
            let mut on_batch = move |batch: &[FoundFile]| {
                stage_cb.extend_from_slice(batch);
                // 回收站阶段不内联文件列表（深度扫描可达数万条，巨型事件会卡死前端），
                // 完整结果在扫描结束后由 get_scan_results 分页拉取。
                emit_cb(ScanEventPayload {
                    status: "scanning".into(), progress: 0.92,
                    message: format!("回收站: 已发现 {} 个已删除文件", stage_cb.len()),
                    found_files: Some((base_count + stage_cb.len()) as u64), speed_mbps: None,
                    session_id: Some(sid_cb.clone()), current_path: None, path_progress: None,
                    result_files: None,
                });
            };
            for drive in &drives {
                if token.is_cancelled() { break; }
                // 该盘符下用户指定的目标目录（可能有多个），无则不过滤
                let filter = target_dirs.iter()
                    .find(|d| d.chars().next().map(|c| c.to_ascii_uppercase()) == Some(*drive))
                    .map(|s| s.as_str());
                scan_recycle_bin(*drive, filter, &mut file_counter, token, &mut on_batch);
            }
        }
        all_files.extend(recycle_stage);

        // 2b: NTFS 卷级已删除文件（解析 $MFT；无权限时降级为提示信息）
        let mut mft_stage: Vec<FoundFile> = Vec::new();
        {
            let emit_cb = &mut *emit;
            let stage_cb = &mut mft_stage;
            let sid_cb = sid.clone();
            let base_count = all_files.len();
            let mut on_ev = move |ev: super::ntfs_deleted::DeletedScanEvent| match ev {
                super::ntfs_deleted::DeletedScanEvent::Batch(b) => {
                    stage_cb.extend_from_slice(b);
                    // 只推计数不内联文件：MFT 深度扫描可达数万条，
                    // 每条事件携带批次会累计成巨型消息流卡死前端。
                    emit_cb(ScanEventPayload {
                        status: "scanning".into(), progress: 0.96,
                        message: format!("深度扫描: 已发现 {} 个已删除文件", stage_cb.len()),
                        found_files: Some((base_count + stage_cb.len()) as u64), speed_mbps: None,
                        session_id: Some(sid_cb.clone()), current_path: None, path_progress: None,
                        result_files: None,
                    });
                }
                super::ntfs_deleted::DeletedScanEvent::Progress(pct, m) => {
                    // 把 MFT 分析进度(0~100)映射到整体进度条的 0.95~0.98 区间，
                    // 让前端进度条在 MFT 扫描阶段实时走动，而非卡死在固定值。
                    let progress = 0.95 + (pct as f32 / 100.0) * 0.03;
                    emit_cb(ScanEventPayload {
                        status: "scanning".into(), progress, message: m,
                        found_files: Some((base_count + stage_cb.len()) as u64), speed_mbps: None,
                        session_id: Some(sid_cb.clone()), current_path: None, path_progress: None,
                        result_files: Some(vec![]),
                    });
                }
            };
            for drive in &drives {
                if token.is_cancelled() { break; }
                // 该盘符下用户指定的目标目录，MFT 扫描据此过滤（重建路径后比对）
                let filter = target_dirs.iter()
                    .find(|d| d.chars().next().map(|c| c.to_ascii_uppercase()) == Some(*drive))
                    .map(|s| s.as_str());
                let outcome = super::ntfs_deleted::scan_volume_deleted(*drive, filter, &mut file_counter, token, &mut on_ev);
                if let Some(n) = outcome.note { notes.push(n); }
            }
        }
        all_files.extend(mft_stage);

        if !notes.is_empty() {
            emit(ScanEventPayload {
                status: "scanning".into(), progress: 0.98,
                message: notes.join("；"),
                found_files: Some(all_files.len() as u64), speed_mbps: None,
                session_id: Some(sid.clone()), current_path: None, path_progress: None,
                result_files: Some(vec![]),
            });
        }
        crate::app_log!("INFO", "深度恢复阶段结束: 新增 {} 个已删除文件（总计 {}）",
            all_files.len() - existing_count, all_files.len());
    }

    let deleted_total = all_files.iter().filter(|f| f.deleted).count();
    let total_count = all_files.len();
    // 完成事件只内联前 EVENT_RESULT_CAP 条预览；完整列表存入后端状态，
    // 前端收到 completed 后通过 get_scan_results 分页拉取，避免数万条记录卡死 UI。
    let preview: Vec<FoundFile> = all_files.iter().take(EVENT_RESULT_CAP).cloned().collect();
    save_results(all_files);
    // 有降级提示（如卷无管理员权限导致强制删除文件没扫）时必须写进完成消息，
    // 否则用户只看「完成! N 个文件」会误以为已把已删除文件都扫出来了。
    let final_message = if notes.is_empty() {
        format!("完成! {} 路径: {} 个现存文件, {} 个可恢复的已删除文件",
            paths.len(), total_count - deleted_total, deleted_total)
    } else {
        format!("完成! {} 路径: {} 个现存文件, {} 个可恢复的已删除文件 | ⚠ {}",
            paths.len(), total_count - deleted_total, deleted_total, notes.join("；"))
    };
    emit(ScanEventPayload {
        status: "completed".into(), progress: 1.0,
        message: final_message,
        found_files: Some(total_count as u64), speed_mbps: None,
        session_id: Some(sid), current_path: None, path_progress: None,
        result_files: Some(preview),
    });

    tokens_cleanup();
}

/// 生产入口：在独立线程中运行，把事件通过 Tauri emit 推给前端。
fn scan_folders_sync<R: tauri::Runtime>(
    app: AppHandle<R>, session_id: String, paths: Vec<String>,
    tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, CancellationToken>>>,
    results: Arc<std::sync::Mutex<std::collections::HashMap<String, Vec<FoundFile>>>>,
) {
    let token = { tokens.lock().unwrap().get(&session_id).cloned() };
    let token = match token {
        Some(t) => t,
        None => {
            crate::app_log!("WARN", "未找到会话 {} 的取消令牌，新建临时令牌", session_id);
            CancellationToken::new()
        }
    };
    let sid = session_id.clone();
    let tokens_ref = tokens.clone();
    let cleanup = move || { tokens_ref.lock().unwrap().remove(&sid); };
    let mut emit = |payload: ScanEventPayload| {
        crate::app_log!("EVENT", "emit scan-event: status={} progress={:.2} files={:?} msg={}",
            payload.status, payload.progress, payload.found_files, payload.message);
        if let Err(e) = app.emit("scan-event", payload) {
            crate::app_log!("ERROR", "emit scan-event 失败: {}", e);
        }
    };
    // 完整结果存入后端状态（只保留最新会话，防止内存无限增长）
    let sid_save = session_id.clone();
    let save_results = move |files: Vec<FoundFile>| {
        let mut map = results.lock().unwrap();
        map.clear();
        crate::app_log!("INFO", "会话 {} 完整结果已保存: {} 个文件", sid_save, files.len());
        map.insert(sid_save.clone(), files);
    };
    run_path_scan_with(&session_id, &paths, &token, &cleanup, true, &mut emit, &save_results);
}

/// 快速检测文件类型（只读前 32 字节，文件不存在或锁定直接返回 Unknown）
fn quick_detect(path: &std::path::Path) -> &'static str {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return "Unknown",
    };
    let mut buf = [0u8; 32];
    match f.read(&mut buf) {
        Ok(n) if n >= 2 => match_signature(&buf[..n]),
        _ => "Unknown",
    }
}

pub fn match_signature(data: &[u8]) -> &'static str {
    if data.len() < 2 { return "Unknown"; }
    match (&data[0], &data[1]) {
        (0xFF, 0xD8) => return "JPEG",
        (0x89, b'P') if data.len() >= 4 && &data[1..4] == b"PNG" => return "PNG",
        (b'G', b'I') if data.len() >= 4 && (&data[2..4] == b"F8" || &data[2..4] == b"F9") => return "GIF",
        (b'B', b'M') => return "BMP",
        (0x25, 0x50) if data.len() >= 4 && &data[2..4] == b"DF" => return "PDF",
        (b'P', b'K') if data.len() >= 4 && &data[2..4] == &[0x03, 0x04] => return "ZIP/DOCX",
        (b'R', b'a') if data.len() >= 4 && &data[2..4] == b"r!" => return "RAR",
        (0x37, 0x7A) => return "7z",
        (0x1F, 0x8B) => return "GZ",
        (b'M', b'Z') => return "PE/EXE",
        (0x7F, b'E') if data.len() >= 4 && &data[2..4] == b"LF" => return "ELF",
        (0x1A, 0x45) => return "MKV",
        (0xFF, 0xFB) | (0xFF, 0xF3) | (0xFF, 0xF2) => return "MP3",
        (b'I', b'D') if data.len() >= 3 && data[2] == b'3' => return "MP3",
        (b'O', b'g') if data.len() >= 4 && &data[2..4] == b"gS" => return "OGG",
        (b'f', b'L') if data.len() >= 4 && &data[2..4] == b"aC" => return "FLAC",
        (b'R', b'I') if data.len() >= 4 && &data[2..4] == b"FF" => return "WAV/AVI",
        (0xD0, 0xCF) => return "OLE/DOC",
        (b'S', b'Q') if data.starts_with(b"SQLite format 3") => return "SQLite",
        (b'B', b'Z') if data.len() >= 3 && data[2] == b'h' => return "BZ2",
        (b'<', b'!') => return "HTML",
        (b'<', b'h') => return "HTML",
        (b'<', b'?') => return "XML",
        (b'{', _) => return "JSON",
        (0x00, 0x00) if data.len() >= 4 && &data[2..4] == &[0x01, 0x00] => return "TTF",
        _ => "Other",
    }
}

// ===== （已删除）模拟磁盘扫描：真实扫描已接入 scan_folders_sync 管线 =====

// ===== 测试 =====
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 创建独立的临时 fixture 目录（每个测试用不同 tag 避免冲突）
    fn fixture_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rf_test_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ----- 路径规范化 -----
    #[test]
    fn normalize_trims_whitespace_and_strips_surrounding_quotes() {
        assert_eq!(normalize_path_input("  D:\\temp  "), "D:\\temp");
        assert_eq!(normalize_path_input("\"D:\\temp\""), "D:\\temp");
        assert_eq!(normalize_path_input("  \"D:\\my folder\"  "), "D:\\my folder");
    }

    #[test]
    fn normalize_empty_input_stays_empty() {
        assert_eq!(normalize_path_input(""), "");
        assert_eq!(normalize_path_input("   "), "");
    }

    #[cfg(windows)]
    #[test]
    fn normalize_unifies_forward_slashes_on_windows() {
        assert_eq!(normalize_path_input("D:/temp/sub"), "D:\\temp\\sub");
    }

    // ----- 路径分类 -----
    #[test]
    fn classify_existing_dir_file_and_missing() {
        let dir = fixture_dir("classify");
        let file = dir.join("a.txt");
        fs::write(&file, b"hello").unwrap();

        assert_eq!(classify_path(dir.to_str().unwrap()), PathKind::Dir);
        assert_eq!(classify_path(file.to_str().unwrap()), PathKind::File);
        assert_eq!(
            classify_path(dir.join("not_exists").to_str().unwrap()),
            PathKind::Missing
        );
        fs::remove_dir_all(&dir).ok();
    }

    // ----- 扫描路径准备 -----
    #[test]
    fn prepare_paths_normalizes_and_separates_invalid() {
        let dir = fixture_dir("prepare");
        let good = format!("  \"{}\"  ", dir.to_str().unwrap());
        let bad = dir.join("missing_dir").to_string_lossy().to_string();

        let (valid, invalid) = prepare_scan_paths(vec![good, bad.clone()]);
        assert_eq!(valid, vec![dir.to_string_lossy().to_string()]);
        assert_eq!(invalid, vec![bad]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prepare_paths_deduplicates() {
        let dir = fixture_dir("dedup");
        let p = dir.to_string_lossy().to_string();
        let (valid, invalid) = prepare_scan_paths(vec![p.clone(), format!(" {} ", p)]);
        assert_eq!(valid.len(), 1);
        assert!(invalid.is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    // ----- 文件收集（扫描引擎核心） -----
    #[test]
    fn collect_files_recurses_dirs_detects_types_and_skips_empty() {
        let dir = fixture_dir("collect");
        fs::write(dir.join("photo.png"), [0x89, b'P', b'N', b'G', 0, 0, 0, 0]).unwrap();
        fs::write(dir.join("empty.txt"), b"").unwrap();
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("doc.pdf"), b"%PDF-1.7").unwrap();

        let mut counter = 0u64;
        let token = CancellationToken::new();
        let files = collect_files(&dir, 0, &mut counter, &token, &mut |_, _| {});

        assert_eq!(files.len(), 2, "空文件应被跳过");
        let png = files.iter().find(|f| f.filename == "photo.png").unwrap();
        assert_eq!(png.file_type, "PNG");
        let pdf = files.iter().find(|f| f.filename == "doc.pdf").unwrap();
        assert_eq!(pdf.file_type, "PDF");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collect_files_accepts_single_file_path() {
        let dir = fixture_dir("single");
        let f = dir.join("one.jpg");
        fs::write(&f, [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();

        let mut counter = 0u64;
        let token = CancellationToken::new();
        let files = collect_files(&f, 0, &mut counter, &token, &mut |_, _| {});

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename, "one.jpg");
        assert_eq!(files[0].file_type, "JPEG");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn collect_files_reports_progress_for_small_dirs_and_batches_large() {
        // 回归用例：小目录（不足 100 文件）也必须至少推送一次进度，
        // 否则前端进度会永远卡在 0%。
        let small = fixture_dir("progress_small");
        for i in 0..3 {
            fs::write(small.join(format!("s{}.dat", i)), b"x").unwrap();
        }
        let mut counter = 0u64;
        let token = CancellationToken::new();
        let mut small_batches = 0u32;
        let files = collect_files(&small, 0, &mut counter, &token, &mut |_total, _batch| {
            small_batches += 1;
        });
        assert_eq!(files.len(), 3);
        assert!(small_batches >= 1, "小目录必须至少推送一次进度，实际 {} 次", small_batches);
        fs::remove_dir_all(&small).ok();

        // 大目录：节流机制下应分批推送，且累计总数正确（批次数量受 200ms 节流影响，只断言下限）
        let dir = fixture_dir("progress");
        for i in 0..250 {
            fs::write(dir.join(format!("f{:03}.dat", i)), b"x").unwrap();
        }

        let mut counter = 0u64;
        let token = CancellationToken::new();
        let mut batches = 0u32;
        let mut last_total = 0u64;
        let files = collect_files(&dir, 0, &mut counter, &token, &mut |total, _batch| {
            batches += 1;
            assert!(total > 0);
            assert!(total >= last_total, "累计计数必须单调递增");
            last_total = total;
        });

        assert_eq!(files.len(), 250);
        assert_eq!(last_total, 250, "最后一次推送的累计数应等于总文件数");
        assert!(batches >= 1, "应至少推送一次进度");
        fs::remove_dir_all(&dir).ok();
    }

    // ----- 取消/暂停（UX-003） -----
    #[test]
    fn collect_files_stops_early_when_cancelled() {
        let dir = fixture_dir("cancel");
        for i in 0..500 {
            fs::write(dir.join(format!("f{:03}.dat", i)), b"x").unwrap();
        }

        let token = CancellationToken::new();
        let mut counter = 0u64;
        // 第一批进度回调时触发取消
        let cancel_trigger = token.clone();
        let files = collect_files(&dir, 0, &mut counter, &token, &mut move |_, _| {
            cancel_trigger.cancel();
        });

        assert!(files.len() < 500, "取消后应立即停止收集，实际收集 {}", files.len());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancellation_token_pause_resume_semantics() {
        let token = CancellationToken::new();
        assert!(!token.is_paused());
        token.pause();
        assert!(token.is_paused());
        token.resume();
        assert!(!token.is_paused());
        // 取消后 wait_while_paused 立即返回 false（不阻塞）
        token.pause();
        token.cancel();
        assert!(!token.wait_while_paused());
    }

    // ----- 文件恢复 -----
    #[test]
    fn recover_one_file_copies_to_output_dir() {
        let src_dir = fixture_dir("recover_src");
        let out_dir = fixture_dir("recover_out");
        let src = src_dir.join("report.pdf");
        fs::write(&src, b"%PDF-data").unwrap();

        let dest = recover_one_file(&src, &out_dir).unwrap();
        assert_eq!(dest, out_dir.join("report.pdf"));
        assert_eq!(fs::read(&dest).unwrap(), b"%PDF-data");

        fs::remove_dir_all(&src_dir).ok();
        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn recover_one_file_renames_on_name_conflict() {
        let src_dir = fixture_dir("recover_src2");
        let out_dir = fixture_dir("recover_out2");
        let src = src_dir.join("a.txt");
        fs::write(&src, b"v1").unwrap();
        // 输出目录已有同名文件
        fs::write(out_dir.join("a.txt"), b"existing").unwrap();

        let dest = recover_one_file(&src, &out_dir).unwrap();
        assert_eq!(dest, out_dir.join("a (1).txt"));
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        // 原有文件不被覆盖
        assert_eq!(fs::read(out_dir.join("a.txt")).unwrap(), b"existing");

        fs::remove_dir_all(&src_dir).ok();
        fs::remove_dir_all(&out_dir).ok();
    }

    #[test]
    fn recover_one_file_fails_for_missing_source() {
        let out_dir = fixture_dir("recover_out3");
        let missing = PathBuf::from("Z:\\definitely\\not\\here.bin");
        assert!(recover_one_file(&missing, &out_dir).is_err());
        fs::remove_dir_all(&out_dir).ok();
    }

    // ----- 真实路径实测（手动运行: cargo test -- --ignored --nocapture） -----
    #[test]
    #[ignore = "需要本机存在 D:\\temp"]
    fn scan_d_temp_real_path() {
        let dir = std::path::Path::new("D:\\temp");
        if !dir.exists() {
            eprintln!("D:\\temp 不存在，跳过");
            return;
        }
        let mut counter = 0u64;
        let token = CancellationToken::new();
        let files = collect_files(dir, 0, &mut counter, &token, &mut |_, _| {});
        eprintln!("D:\\temp 扫描到 {} 个文件", files.len());
        for f in files.iter().take(10) {
            eprintln!("  [{}] {} ({}B)", f.file_type, f.filename, f.size_bytes);
        }
        assert_eq!(files.len() as u64, counter);
    }

    // ----- 端到端：扫描编排事件流（无需 GUI，验证进度不卡 0） -----
    #[test]
    fn run_path_scan_emits_progress_and_completed_events() {
        let dir = fixture_dir("e2e_events");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        for i in 0..5 {
            fs::write(dir.join(format!("a{}.txt", i)), b"hello world").unwrap();
        }
        fs::write(sub.join("b.png"), [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();

        let token = CancellationToken::new();
        let mut events: Vec<ScanEventPayload> = Vec::new();
        let mut emit = |p: ScanEventPayload| events.push(p);
        let cleaned = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cleaned_cb = cleaned.clone();
        let cleanup = move || { cleaned_cb.store(true, std::sync::atomic::Ordering::SeqCst); };

        run_path_scan_with("e2e-1", &[dir.to_string_lossy().to_string()], &token, &cleanup, false, &mut emit, &|_| {});

        // 1) 事件流完整：首事件 -> 至少一次中间进度 -> 完成事件
        assert!(events.len() >= 3, "事件数不足: {}", events.len());
        assert_eq!(events.first().unwrap().status, "scanning");
        let last = events.last().unwrap();
        assert_eq!(last.status, "completed", "末事件必须是 completed");
        assert!((last.progress - 1.0).abs() < f32::EPSILON, "完成时进度必须为 100%");
        assert!(cleaned.load(std::sync::atomic::Ordering::SeqCst), "结束后必须清理 token");

        // 2) 核心回归断言：扫描期间必须存在 > 0 的进度事件（修复卡 0 的 Bug）
        let mid_progress: Vec<f32> = events.iter()
            .filter(|e| e.status == "scanning")
            .map(|e| e.progress).collect();
        assert!(mid_progress.iter().any(|p| *p > 0.0),
            "扫描期间没有任何 >0 的进度事件，进度会卡 0: {:?}", mid_progress);

        // 3) 完成事件携带全部结果文件，且计数正确（6 = 5 txt + 1 png）
        let results = last.result_files.as_ref().expect("completed 事件必须携带结果文件");
        assert_eq!(results.len(), 6, "结果文件数不对");
        assert_eq!(last.found_files, Some(6));

        fs::remove_dir_all(&dir).ok();
    }

    // ----- 真实路径端到端：扫描 D:\temp -> 发现文件 -> 实际恢复 -> 逐字节校验 -----
    // 手动运行: cargo test -p rescueforge --lib e2e_scan_and_recover_d_temp -- --ignored --nocapture
    #[test]
    #[ignore = "需要本机存在 D:\\temp"]
    fn e2e_scan_and_recover_d_temp() {
        let dir = std::path::Path::new("D:\\temp");
        if !dir.exists() {
            eprintln!("D:\\temp 不存在，跳过");
            return;
        }

        // 阶段 1：跑完整扫描编排，收集事件流（与生产代码同一路径）
        let token = CancellationToken::new();
        let mut events: Vec<ScanEventPayload> = Vec::new();
        let mut emit = |p: ScanEventPayload| events.push(p);
        let cleanup = || {};
        run_path_scan_with("e2e-dtemp", &["D:\\temp".to_string()], &token, &cleanup, false, &mut emit, &|_| {});

        eprintln!("=== 扫描阶段：共 {} 个事件 ===", events.len());
        for e in &events {
            eprintln!("  [{}] progress={:.2} files={:?} {}",
                e.status, e.progress, e.found_files, e.message);
        }

        let last = events.last().expect("必须收到完成事件");
        assert_eq!(last.status, "completed", "扫描未正常完成");
        assert!((last.progress - 1.0).abs() < f32::EPSILON);
        assert!(events.iter().filter(|e| e.status == "scanning").any(|e| e.progress > 0.0),
            "扫描期间进度始终为 0，前端会卡 0%");
        let found = last.result_files.as_ref().expect("completed 事件无结果文件");
        assert!(!found.is_empty(), "D:\\temp 未扫描到任何文件");
        eprintln!("=== 扫描完成：发现 {} 个文件 ===", found.len());

        // 阶段 2：模拟前端 ResultsView 的真实调用：id||filepath 编码 -> recover_files_core（与命令同一逻辑）
        let out_dir = std::env::temp_dir().join(format!("rf_e2e_recover_{}", std::process::id()));
        let _ = fs::remove_dir_all(&out_dir);

        let ids: Vec<String> = found.iter()
            .map(|f| format!("{}||{}", f.id, f.filepath))
            .collect();
        let mut recover_events: Vec<ScanEventPayload> = Vec::new();
        let mut rec_emit = |p: ScanEventPayload| recover_events.push(p);
        let result = recover_files_core(&ids, &out_dir.to_string_lossy(), &mut rec_emit)
            .expect("recover_files_core 不应报错");

        let ok = result.get("success").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let failed = result.get("failed").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        // 阶段 3：逐字节校验每个恢复出的文件与源文件一致（完整性）
        for f in found {
            let src = std::path::Path::new(&f.filepath);
            let src_bytes = fs::read(src).unwrap();
            // 在输出目录中定位恢复产物（可能有重名后缀）
            let stem = std::path::Path::new(&f.filename).file_stem()
                .map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let dest = fs::read_dir(&out_dir).unwrap().flatten()
                .map(|e| e.path())
                .find(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with(stem.as_str())))
                .unwrap_or_else(|| panic!("输出目录找不到 {} 的恢复产物", f.filename));
            let dst_bytes = fs::read(&dest).unwrap();
            assert_eq!(src_bytes.len(), dst_bytes.len(), "恢复后大小不一致: {}", dest.display());
            assert_eq!(src_bytes, dst_bytes, "恢复后内容不一致: {}", dest.display());
            eprintln!("  [校验通过] {} -> {} ({}B)", f.filename, dest.display(), dst_bytes.len());
        }

        eprintln!("=== 恢复阶段：成功 {} / 失败 {} ，恢复事件 {} 个，输出目录 {} ===",
            ok, failed, recover_events.len(), out_dir.display());
        assert_eq!(failed, 0, "存在恢复失败的文件");
        assert_eq!(ok, found.len(), "恢复成功数应等于扫描发现数");
        assert_eq!(recover_events.len(), found.len(), "每个文件都应推送一条恢复进度事件");
        fs::remove_dir_all(&out_dir).ok();
    }

    // ----- 深度恢复：盘符提取与回收站元数据解析 -----
    #[test]
    fn drive_letters_dedup_and_uppercase() {
        assert_eq!(
            drive_letters_of(&["D:\\temp".into(), "d:\\foo".into(), "E:\\".into()]),
            vec!['D', 'E']
        );
        let none: Vec<String> = Vec::new();
        assert!(drive_letters_of(&none).is_empty());
        assert!(drive_letters_of(&["relative/path".into(), "/unix/path".into()]).is_empty());
    }

    #[test]
    fn parse_recycle_meta_v2_and_v1() {
        // v2（Win8+）：头部版本 2，0x18 起 4 字节路径字符数，0x1C 起 UTF-16LE 路径
        let ft: u64 = (1_700_000_000u64 + 11_644_473_600) * 10_000_000;
        let mut buf = vec![0u8; 28];
        buf[0..8].copy_from_slice(&2u64.to_le_bytes());
        buf[8..16].copy_from_slice(&555u64.to_le_bytes());
        buf[16..24].copy_from_slice(&ft.to_le_bytes());
        let path = "D:\\docs\\report.docx";
        let wide: Vec<u16> = path.encode_utf16().collect();
        // 长度字段是 4 字节：高 2 字节为 0、低 2 字节为字符数（真实 $I 即如此布局）
        buf[24..28].copy_from_slice(&(wide.len() as u32).to_le_bytes());
        for c in &wide { buf.extend_from_slice(&c.to_le_bytes()); }
        let (p, unix) = parse_recycle_info_meta(&buf).expect("v2 元数据必须可解析");
        assert_eq!(p, path, "解析出的路径不得含前导 null 等脏字符");
        assert!(!p.starts_with('\0'), "路径开头不得有 null");
        assert_eq!(unix, 1_700_000_000, "删除时间换算错误");

        // 回归用例：按真实 $I 布局（长度字段高 2 字节为 0）构造时，
        // 若仍按「2字节长度+偏移26」解析会多读一个前导 null，必须杜绝。
        // 上面 buf[26..28] 正是 0，若解析错会产出 "\0D:\docs\report.docx"。
        assert!(crate::commands::ntfs_deleted::path_under_dir(&p, "D:\\docs"),
            "解析出的路径必须能被目录过滤正确匹配");

        // 路径含结尾 null 终止符（部分系统把终止符计入长度）时必须裁掉
        let mut buf2 = buf.clone();
        buf2.extend_from_slice(&0u16.to_le_bytes());
        let n = u32::from_le_bytes(buf2[24..28].try_into().unwrap()) as usize + 1;
        buf2[24..28].copy_from_slice(&(n as u32).to_le_bytes());
        let (p2, _) = parse_recycle_info_meta(&buf2).expect("含终止符时必须可解析");
        assert_eq!(p2, path, "结尾 null 必须被裁掉");

        // v1（Win7）：版本 1，固定 260 个 UTF-16 字符，空字符结尾
        let mut b1 = vec![0u8; 24 + 520];
        b1[0..8].copy_from_slice(&1u64.to_le_bytes());
        b1[16..24].copy_from_slice(&ft.to_le_bytes());
        for (i, c) in "C:\\a.txt".encode_utf16().enumerate() {
            b1[24 + i * 2..26 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        let (p1, _) = parse_recycle_info_meta(&b1).expect("v1 元数据必须可解析");
        assert_eq!(p1, "C:\\a.txt");

        assert!(parse_recycle_info_meta(&[0u8; 10]).is_none(), "过短缓冲必须拒绝");
    }

    // ----- 深度恢复：恢复分发（recycle:// 与 ntfs:// 编码） -----
    #[test]
    fn recover_dispatch_handles_recycle_and_ntfs_encodings() {
        let dir = fixture_dir("dispatch");
        let out = fixture_dir("dispatch_out");
        // 构造回收站数据文件（模拟 $R）
        let rdir = dir.join("SID");
        fs::create_dir_all(&rdir).unwrap();
        let rfile = rdir.join("$RABC.txt");
        fs::write(&rfile, b"deleted-data").unwrap();

        let ids = vec![
            format!("f-1||recycle://{}||report-final.txt", rfile.to_string_lossy()),
            // ntfs:// 非法记录号：必须优雅失败计入 failed，绝不允许 panic 或整体报错
            "f-2||ntfs://D:/999999999".to_string(),
        ];
        let mut evs: Vec<ScanEventPayload> = Vec::new();
        let mut emit = |p: ScanEventPayload| evs.push(p);
        let res = recover_files_core(&ids, &out.to_string_lossy(), &mut emit).expect("分发层不应整体报错");
        assert_eq!(res.get("success").and_then(|v| v.as_u64()), Some(1), "回收站文件必须恢复成功");
        assert_eq!(res.get("failed").and_then(|v| v.as_u64()), Some(1), "无效 MFT 记录必须计入失败");
        // 回收站恢复产物必须按原始文件名落盘且内容一致
        assert_eq!(fs::read(out.join("report-final.txt")).unwrap(), b"deleted-data");
        assert_eq!(evs.len(), 2, "每个文件都应推送恢复进度事件");
        fs::remove_dir_all(&dir).ok();
        fs::remove_dir_all(&out).ok();
    }

    #[test]
    fn deep_phase_completes_gracefully_on_unavailable_volume() {
        // deep:true + 不存在的盘符 X:：回收站读取失败返回空、卷打开失败降级为提示，
        // 不得 panic，事件流必须以 completed 正常收尾。
        let token = CancellationToken::new();
        let mut events: Vec<ScanEventPayload> = Vec::new();
        let mut emit = |p: ScanEventPayload| events.push(p);
        let cleanup = || {};
        run_path_scan_with("deep-1", &["X:\\rf_nonexistent_dir".to_string()], &token, &cleanup, true, &mut emit, &|_| {});
        let last = events.last().expect("必须收到完成事件");
        assert_eq!(last.status, "completed");
        assert!(events.iter().any(|e| e.message.contains("深度恢复阶段")),
            "必须出现深度恢复阶段事件");
        assert!(events.iter().any(|e| e.message.contains("X")),
            "卷不可用时必须把原因提示给前端");
    }

    // ----- 真实卷深度扫描实测（手动运行，管理员终端效果最佳） -----
    // cargo test -p rescueforge --lib e2e_deep_scan_d_deleted_files -- --ignored --nocapture
    #[test]
    #[ignore = "需要本机存在 D: 卷"]
    fn e2e_deep_scan_d_deleted_files() {
        // 自给自足：现场制造删除素材并用 Win32 DeleteFileW 删除（句柄立即关闭，
        // MFT 记录当场释放）。注意：素材必须在扫描前删除，且删除后不能再有大量
        // 写盘（否则记录会被新文件重用——这正是真实数据恢复的规律）。
        let mat_path = "D:\\temp\\rf_deep_test_deleted.txt";
        let content = format!("RESCUEFORGE-DEEP-RECOVERY-{}\n深度恢复测试素材", std::process::id());
        fs::write(mat_path, &content).expect("制造删除素材失败");
        #[cfg(windows)]
        {
            use windows::Win32::Storage::FileSystem::DeleteFileW;
            use windows::core::PCWSTR;
            let wide: Vec<u16> = mat_path.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { DeleteFileW(PCWSTR::from_raw(wide.as_ptr())).expect("DeleteFileW 删除素材失败"); }
        }
        #[cfg(not(windows))]
        { fs::remove_file(mat_path).ok(); }
        assert!(!std::path::Path::new(mat_path).exists(), "素材必须已从文件系统移除");

        let token = CancellationToken::new();
        let mut events: Vec<ScanEventPayload> = Vec::new();
        let mut emit = |p: ScanEventPayload| events.push(p);
        let cleanup = || {};
        run_path_scan_with("e2e-deep-d", &["D:\\temp".to_string()], &token, &cleanup, true, &mut emit, &|_| {});
        let last = events.last().expect("必须收到完成事件");
        assert_eq!(last.status, "completed");
        let files = last.result_files.as_ref().expect("completed 必须携带结果");
        let deleted: Vec<&FoundFile> = files.iter().filter(|f| f.deleted).collect();
        eprintln!("=== 深度扫描 D: 总计 {} 个文件，其中已删除 {} 个 ===", files.len(), deleted.len());
        for f in deleted.iter().take(20) {
            eprintln!("  [已删除] {} ({}B) mtime={:?} {}", f.filename, f.size_bytes, f.modified_time, f.filepath);
        }
        // 必须发现本轮制造的删除素材（MFT 来源）
        let material = deleted.iter().find(|f| f.filename == "rf_deep_test_deleted.txt")
            .expect("必须通过 MFT 发现已删除素材 rf_deep_test_deleted.txt");
        assert!(material.filepath.starts_with(super::super::ntfs_deleted::NTFS_PREFIX),
            "素材应来自 MFT 卷直读通道，实际: {}", material.filepath);
        assert!(material.size_bytes > 0, "素材大小必须 > 0");
        eprintln!("=== 命中素材: {} ({}B) {} ===", material.filename, material.size_bytes, material.filepath);
        // 剪切素材：同卷移动会复用 MFT 记录（不会留下已删除记录），若意外留下也允许被发现，仅记录
        if let Some(cut) = deleted.iter().find(|f| f.filename == "rf_deep_test_cut.txt") {
            eprintln!("=== 剪切素材也在已删除列表: {} ===", cut.filepath);
        }
        // MFT 来源实际恢复 + 内容校验（需要管理员权限运行本测试）
        {
            let out = std::env::temp_dir().join(format!("rf_deep_recover_{}", std::process::id()));
            let _ = fs::remove_dir_all(&out);
            let ids = vec![format!("{}||{}", material.id, material.filepath)];
            let mut rev: Vec<ScanEventPayload> = Vec::new();
            let mut re = |p: ScanEventPayload| rev.push(p);
            let res = recover_files_core(&ids, &out.to_string_lossy(), &mut re).expect("恢复分发不应整体报错");
            eprintln!("=== MFT 恢复结果: {} ===", res);
            assert!(res.get("success").and_then(|v| v.as_u64()) == Some(1), "MFT 来源恢复必须成功: {}", res);
            let recovered = out.join("rf_deep_test_deleted.txt");
            let meta = fs::metadata(&recovered).expect("恢复文件必须落盘");
            assert_eq!(meta.len(), material.size_bytes, "恢复内容大小必须一致");
            let recovered_content = fs::read_to_string(&recovered).unwrap_or_default();
            assert!(recovered_content.contains("RESCUEFORGE-DEEP-RECOVERY"),
                "恢复内容必须与删除前一致，实际: {:?}", recovered_content);
            eprintln!("=== MFT 恢复内容校验通过 ({}B): {} ===", meta.len(), recovered_content.lines().next().unwrap_or(""));
            fs::remove_dir_all(&out).ok();
        }
        // 另挑一个回收站来源小文件实际恢复；无管理员时 MFT 来源会失败，回收站来源无需管理员
        if let Some(f) = deleted.iter().find(|f| f.size_bytes > 0 && f.size_bytes < 10 * 1024 * 1024) {
            let out = std::env::temp_dir().join(format!("rf_deep_recover_{}", std::process::id()));
            let _ = fs::remove_dir_all(&out);
            let ids = vec![format!("{}||{}", f.id, f.filepath)];
            let mut rev: Vec<ScanEventPayload> = Vec::new();
            let mut re = |p: ScanEventPayload| rev.push(p);
            let res = recover_files_core(&ids, &out.to_string_lossy(), &mut re).expect("恢复分发不应整体报错");
            eprintln!("=== 恢复结果: {} ===", res);
            if let Ok(rd) = fs::read_dir(&out) {
                for e in rd.flatten() {
                    eprintln!("  [落盘] {} ({}B)", e.path().display(), e.metadata().map(|m| m.len()).unwrap_or(0));
                }
            }
            fs::remove_dir_all(&out).ok();
        }
    }

    /// 诊断：回收站文件为何没显示。回收站扫描无需管理员权限，可直接运行。
    /// 依次验证：目录可访问性 → $I 数量 → 原始路径解析 → 按目标目录过滤后剩余。
    /// cargo test -p rescueforge --lib diag_recycle_bin -- --ignored --nocapture
    #[test]
    #[ignore = "诊断用"]
    fn diag_recycle_bin() {
        for drive in ['C', 'D'] {
            let base = format!("{}:\\$RECYCLE.BIN", drive);
            let sid_dirs = match std::fs::read_dir(&base) {
                Ok(e) => e,
                Err(e) => { eprintln!("[RB-{}] 无法读取 {}: {}", drive, base, e); continue; }
            };
            eprintln!("[RB-{}] {} 可访问", drive, base);
            let mut total_i = 0usize;
            for sid in sid_dirs.flatten() {
                if !sid.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
                let entries = match std::fs::read_dir(sid.path()) {
                    Ok(e) => e,
                    Err(e) => { eprintln!("[RB-{}]   无法读 SID 目录 {}: {}", drive, sid.path().display(), e); continue; }
                };
                for entry in entries.flatten() {
                    let ename = entry.file_name().to_string_lossy().to_string();
                    if !ename.starts_with("$I") || ename.len() < 3 { continue; }
                    total_i += 1;
                    let meta = match std::fs::read(entry.path()) {
                        Ok(m) => m,
                        Err(e) => { eprintln!("[RB-{}]   {} 读取失败: {}", drive, ename, e); continue; }
                    };
                    let (orig, _unix) = match parse_recycle_info_meta(&meta) {
                        Some(x) => x,
                        None => { eprintln!("[RB-{}]   {} 元数据解析失败", drive, ename); continue; }
                    };
                    // $R 数据文件是否还在（被清空则不存在）
                    let r_path = sid.path().join(format!("$R{}", &ename[2..]));
                    let r_ok = r_path.is_file();
                    let r_size = r_path.metadata().map(|m| m.len()).unwrap_or(0);
                    let under_temp = crate::commands::ntfs_deleted::path_under_dir(&orig, "D:\\temp");
                    eprintln!("[RB-{}]   orig={} | $R存在={} 大小={} | 在D:\\temp下={}",
                        drive, orig, r_ok, r_size, under_temp);
                    // 关键：打印原始字节，定位路径里是否混入不可见字符（如 UTF-16 结尾 \0）
                    eprintln!("[RB-DBG]   len={} bytes={:?}", orig.len(), orig.as_bytes());
                    let cleaned = orig.trim_matches('\0').trim();
                    eprintln!("[RB-DBG]   cleaned={:?} under={}",
                        cleaned, crate::commands::ntfs_deleted::path_under_dir(cleaned, "D:\\temp"));
                    if total_i >= 20 { break; }
                }
            }
            eprintln!("[RB-{}] 共检查 {} 个 $I 元数据", drive, total_i);
        }

        // 实际跑一遍带过滤 / 不带过滤的回收站扫描，对比数量
        let token = CancellationToken::new();
        let mut c1 = 0u64;
        let all = scan_recycle_bin('D', None, &mut c1, &token, &mut |_| {});
        eprintln!("[RB-SCAN] 不过滤: {} 个", all.len());
        let mut c2 = 0u64;
        let filtered = scan_recycle_bin('D', Some("D:\\temp"), &mut c2, &token, &mut |_| {});
        eprintln!("[RB-SCAN] 过滤 D:\\temp: {} 个", filtered.len());
        for f in filtered.iter().take(10) {
            eprintln!("[RB-SCAN]   {} <- {:?}", f.filename, f.original_path);
        }
    }
}
