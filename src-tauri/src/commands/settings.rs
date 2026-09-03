use tauri::State;
use crate::database::Database;

/// 获取用户设置
#[tauri::command]
pub async fn get_settings(db: State<'_, Database>) -> Result<serde_json::Value, String> {
    // Read all settings from SQLite
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings")
        .map_err(|e| e.to_string())?;
    
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }).map_err(|e| e.to_string())?;

    let mut map = serde_json::Map::new();
    for row in rows {
        let (key, value) = row.map_err(|e| e.to_string())?;
        // Try parse as JSON, fallback to string
        let parsed: serde_json::Value = serde_json::from_str(&value).unwrap_or(serde_json::Value::String(value));
        map.insert(key, parsed);
    }

    // Ensure defaults if no settings exist
    if map.is_empty() {
        map.insert("theme".into(), serde_json::json!("eye"));
        map.insert("language".into(), serde_json::json!("zh-CN"));
        map.insert("default_recovery_path".into(), serde_json::json!(""));
        map.insert("auto_update".into(), serde_json::json!(true));
        map.insert("show_hidden_files".into(), serde_json::json!(false));
    }

    Ok(serde_json::Value::Object(map))
}

/// 更新用户设置
#[tauri::command]
pub async fn update_settings(
    db: State<'_, Database>,
    settings: std::collections::HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    for (key, value) in settings {
        let value_str = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        db.save_setting(&key, &value_str).map_err(|e| e.to_string())?;
    }
    Ok(())
}
