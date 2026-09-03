// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod single_instance;

fn main() {
    // 单实例守卫：提权/普通实例混跑会导致 WebView2 用户数据目录冲突，
    // 后启动的实例前端完全不挂载（白屏）。检测到已有实例时激活其窗口并退出。
    #[cfg(target_os = "windows")]
    {
        let takeover = std::env::args().any(|a| a == single_instance::TAKEOVER_ARG);
        if !single_instance::acquire(takeover) {
            return;
        }
    }
    rescueforge_lib::run()
}
