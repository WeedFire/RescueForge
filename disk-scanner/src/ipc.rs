use rescueforge_core::events::SidecarMessage;
use serde_json;

/// JSON Lines IPC 协议工具
pub fn format_message(msg: &SidecarMessage) -> String {
    serde_json::to_string(msg).unwrap_or_else(|_| {
        serde_json::to_string(&SidecarMessage::Error {
            message: "Serialization failed".into(),
        })
        .unwrap()
    })
}

/// 解析 Sidecar 命令
pub fn parse_command(line: &str) -> Result<rescueforge_core::events::SidecarCommand, String> {
    serde_json::from_str(line).map_err(|e| format!("Invalid command: {}", e))
}
