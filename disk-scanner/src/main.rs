mod raw_disk;
mod partition;
mod smart;
mod ntfs;
mod fat;
mod ext4;
mod carving;
mod partition_rebuild;
mod boot_sector;
mod disk_image;
mod ipc;

use std::io::{self, BufRead, Write};
use rescueforge_core::events::SidecarCommand;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to read stdin: {}", e);
                continue;
            }
        };

        let command: SidecarCommand = match serde_json::from_str(&line) {
            Ok(cmd) => cmd,
            Err(e) => {
                let error_msg = rescueforge_core::events::SidecarMessage::Error {
                    message: format!("Invalid command: {}", e),
                };
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", serde_json::to_string(&error_msg).unwrap());
                out.flush().ok();
                continue;
            }
        };

        match command {
            SidecarCommand::Scan { device_path, scan_mode, signature_ids: _ } => {
                let log_msg = rescueforge_core::events::SidecarMessage::Log {
                    level: "info".into(),
                    message: format!("Starting scan on {} (mode: {})", device_path, scan_mode),
                };
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", serde_json::to_string(&log_msg).unwrap());
                out.flush().ok();
            }
            SidecarCommand::Pause => {
                // TODO
            }
            SidecarCommand::Resume => {
                // TODO
            }
            SidecarCommand::Cancel => {
                break;
            }
        }
    }
}
