pub mod commands;
pub mod privilege;
pub mod scheduler;
pub mod database;
pub mod sidecar;
pub mod log;

use rescueforge_core::events::ScanEvent;
use serde::Serialize;
use std::sync::Arc;
use commands::scan::FoundFile;

/// 简化的扫描事件（用于 Tauri emit）
#[derive(Debug, Clone, Serialize)]
pub struct ScanEventPayload {
    pub status: String,
    pub progress: f32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_files: Option<Vec<FoundFile>>,
}

impl From<ScanEvent> for ScanEventPayload {
    fn from(event: ScanEvent) -> Self {
        Self {
            status: event.status,
            progress: event.progress,
            message: event.message,
            found_files: event.found_files,
            speed_mbps: event.speed_mbps,
            session_id: event.session_id,
            current_path: None,
            path_progress: None,
            result_files: None,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use database::Database;

    // 标注构建模式：dev 模式（cargo tauri dev 编译）的 exe 内嵌 devUrl（http://localhost:1420），
    // 脱离 dev server 运行（如提权重启）会白屏，日志标注便于排查。
    if cfg!(dev) {
        crate::app_log!("WARN", "==== RescueForge 启动 (DEV 模式, 依赖 localhost dev server), 日志文件: {} ====", log::log_path().display());
    } else {
        crate::app_log!("INFO", "==== RescueForge 启动 (内嵌前端), 日志文件: {} ====", log::log_path().display());
    }

    // Initialize database in app data directory
    let db_path = get_db_path();
    let db = Database::new(&db_path).expect("Failed to initialize database");

    // Initialize scheduler state (delayed start in setup)
    let scheduler = Arc::new(std::sync::Mutex::new(scheduler::TaskScheduler::new()));
    let scheduler_for_setup = scheduler.clone();
    let scheduler_state = commands::scan::SchedulerState {
        scheduler: scheduler,
        active_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        session_results: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(db)
        .manage(scheduler_state)
        .setup(move |_app| {
            // start() uses std::thread::spawn internally, no Tokio context needed
            scheduler_for_setup.lock().unwrap().start();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::disk::get_disks,
            commands::disk::get_smart_status,
            commands::scan::select_folder,
            commands::scan::validate_path,
            commands::scan::reveal_in_explorer,
            commands::scan::start_path_scan,
            commands::scan::start_scan,
            commands::scan::pause_scan,
            commands::scan::resume_scan,
            commands::scan::cancel_scan,
            commands::scan::recover_files,
            commands::scan::preview_file,
            commands::scan::get_file_info,
            commands::scan::get_scan_results,
            commands::partition::search_lost_partitions,
            commands::partition::rebuild_partition_table,
            commands::partition::backup_boot_sector,
            commands::partition::restore_boot_sector,
            commands::partition::create_disk_image,
            commands::partition::cancel_disk_image,
            commands::system::get_privilege_status,
            commands::system::restart_elevated,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::applog::get_app_log,
            commands::applog::write_app_log,
            commands::applog::get_autotest_flag,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RescueForge");
}

fn get_db_path() -> String {
    // Use app data directory for database storage
    if let Ok(data_dir) = std::env::var("APPDATA") {
        let dir = format!("{}\\RescueForge", data_dir);
        let _ = std::fs::create_dir_all(&dir);
        return format!("{}\\rescueforge.db", dir);
    }
    "rescueforge.db".to_string()
}
