use rusqlite::{Connection, Result as SqlResult};
use std::sync::Mutex;

/// 数据库管理器
pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    /// 初始化数据库，创建表结构
    pub fn new(db_path: &str) -> SqlResult<Self> {
        let conn = Connection::open(db_path)?;

        // 启用 WAL 模式提升并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        // 创建核心表
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS scan_sessions (
                id TEXT PRIMARY KEY,
                disk_index INTEGER NOT NULL,
                scan_mode TEXT NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT,
                total_sectors INTEGER NOT NULL DEFAULT 0,
                scanned_sectors INTEGER NOT NULL DEFAULT 0,
                found_files INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                progress REAL NOT NULL DEFAULT 0,
                speed_mbps REAL NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS recovered_files (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                filename TEXT NOT NULL,
                extension TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                original_path TEXT,
                recovered_path TEXT,
                signature_type TEXT,
                start_sector INTEGER NOT NULL,
                sha256 TEXT,
                status TEXT NOT NULL DEFAULT 'found',
                preview_available INTEGER NOT NULL DEFAULT 0,
                created_time TEXT,
                modified_time TEXT,
                FOREIGN KEY (session_id) REFERENCES scan_sessions(id)
            );

            CREATE TABLE IF NOT EXISTS file_signatures (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                extension TEXT NOT NULL UNIQUE,
                description TEXT,
                header_bytes BLOB NOT NULL,
                footer_bytes BLOB,
                max_size_bytes INTEGER,
                enabled INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_recovered_files_session
                ON recovered_files(session_id);
            CREATE INDEX IF NOT EXISTS idx_recovered_files_extension
                ON recovered_files(extension);
            CREATE INDEX IF NOT EXISTS idx_scan_sessions_status
                ON scan_sessions(status);"
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 保存设置
    pub fn save_setting(&self, key: &str, value: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }

    /// 读取设置
    pub fn get_setting(&self, key: &str) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map([key], |row| row.get(0))?;
        match rows.next() {
            Some(Ok(val)) => Ok(Some(val)),
            _ => Ok(None),
        }
    }
}
