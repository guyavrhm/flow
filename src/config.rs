use rusqlite::{Connection, Result, params};
use std::fs;
use crate::paths::get_db_path;
use once_cell::sync::Lazy;
use std::sync::RwLock;

static MONITOR_LAYOUT_CACHE: Lazy<RwLock<Option<Vec<MonitorLayout>>>> = Lazy::new(|| RwLock::new(None));

pub const PC_SERVER: i32 = 1;
pub const PC_CLIENT: i32 = 0;
pub const ENCRYPTION_ON: i32 = 1;
pub const ENCRYPTION_OFF: i32 = 0;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettingsData {
    pub ip: String,
    pub password: String,
    pub pc: i32,
    pub encryption: i32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScreenAttachments {
    pub address: String,
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
}

pub fn initialize_db() -> Result<()> {
    let db_path = get_db_path();
    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let conn = Connection::open(&db_path)?;

    // Create settings table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            IP TEXT,
            password TEXT,
            pc INTEGER,
            encryption INTEGER
        )",
        [],
    )?;

    // Check if settings table is empty
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))?;

    if count == 0 {
        conn.execute(
            "INSERT INTO settings (IP, password, pc, encryption) VALUES (?, ?, ?, ?)",
            params!["", "", PC_SERVER, ENCRYPTION_OFF],
        )?;
    }

    // Create screens table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS screens (
            address TEXT UNIQUE,
            top TEXT,
            right TEXT,
            bottom TEXT,
            left TEXT
        )",
        [],
    )?;

    // Add default main screen if screens table is empty
    let screen_count: i64 = conn.query_row("SELECT COUNT(*) FROM screens", [], |row| row.get(0))?;

    if screen_count == 0 {
        conn.execute(
            "INSERT INTO screens (address, top, right, bottom, left) VALUES (?, NULL, NULL, NULL, NULL)",
            params!["main"],
        )?;
    }

    // Create monitors table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS monitors (
            monitor_id TEXT PRIMARY KEY,
            host TEXT NOT NULL,
            monitor_name TEXT NOT NULL,
            x INTEGER NOT NULL,
            y INTEGER NOT NULL,
            width INTEGER NOT NULL,
            height INTEGER NOT NULL,
            scale_factor REAL NOT NULL,
            local_x INTEGER NOT NULL,
            local_y INTEGER NOT NULL
        )",
        [],
    )?;

    // Create known_hosts table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS known_hosts (
            address TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            trusted_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    // Prune obsolete loopback 127.0.0.1 entries from previous local test runs to prevent duplicate screen ghosts
    let _ = conn.execute("DELETE FROM monitors WHERE host = '127.0.0.1'", []);
    let _ = conn.execute("DELETE FROM screens WHERE address = '127.0.0.1'", []);

    Ok(())
}

pub fn get_trusted_fingerprint(address: &str) -> Result<Option<String>> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    let mut stmt = conn.prepare("SELECT fingerprint FROM known_hosts WHERE address = ?")?;
    let mut rows = stmt.query(params![address])?;
    if let Some(row) = rows.next()? {
        let fp: String = row.get(0)?;
        Ok(Some(fp))
    } else {
        Ok(None)
    }
}

pub fn trust_fingerprint(address: &str, fingerprint: &str) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "INSERT OR REPLACE INTO known_hosts (address, fingerprint) VALUES (?, ?)",
        params![address, fingerprint],
    )?;
    Ok(())
}

pub fn untrust_fingerprint(address: &str) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute("DELETE FROM known_hosts WHERE address = ?", params![address])?;
    Ok(())
}


pub fn get_settings() -> Result<SettingsData> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.query_row(
        "SELECT IP, password, pc, encryption FROM settings LIMIT 1",
        [],
        |row| {
            Ok(SettingsData {
                ip: row.get(0)?,
                password: row.get(1)?,
                pc: row.get(2)?,
                encryption: row.get(3)?,
            })
        },
    )
}

pub fn save_settings(settings: &SettingsData) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "UPDATE settings SET IP = ?, password = ?, pc = ?, encryption = ?",
        params![
            settings.ip,
            settings.password,
            settings.pc,
            settings.encryption
        ],
    )?;
    Ok(())
}

pub fn get_screens() -> Result<Vec<ScreenAttachments>> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    let mut stmt = conn.prepare("SELECT address, top, right, bottom, left FROM screens")?;
    let screen_iter = stmt.query_map([], |row| {
        Ok(ScreenAttachments {
            address: row.get(0)?,
            top: row.get(1)?,
            right: row.get(2)?,
            bottom: row.get(3)?,
            left: row.get(4)?,
        })
    })?;

    let mut list = Vec::new();
    for s in screen_iter {
        list.push(s?);
    }
    Ok(list)
}

pub fn get_attachments(name: &str) -> Result<ScreenAttachments> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    let res = conn.query_row(
        "SELECT address, top, right, bottom, left FROM screens WHERE address = ?",
        params![name],
        |row| {
            Ok(ScreenAttachments {
                address: row.get(0)?,
                top: row.get(1)?,
                right: row.get(2)?,
                bottom: row.get(3)?,
                left: row.get(4)?,
            })
        },
    );

    match res {
        Ok(attachments) => Ok(attachments),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Add new screen with null attachments
            conn.execute(
                "INSERT INTO screens (address, top, right, bottom, left) VALUES (?, NULL, NULL, NULL, NULL)",
                params![name],
            )?;
            Ok(ScreenAttachments {
                address: name.to_string(),
                top: None,
                right: None,
                bottom: None,
                left: None,
            })
        }
        Err(e) => Err(e),
    }
}

pub fn update_screen(name: &str, attachments: &ScreenAttachments) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "UPDATE screens SET top = ?, right = ?, bottom = ?, left = ? WHERE address = ?",
        params![
            attachments.top,
            attachments.right,
            attachments.bottom,
            attachments.left,
            name
        ],
    )?;
    Ok(())
}

pub fn remove_screen(name: &str) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute("DELETE FROM screens WHERE address = ?", params![name])?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitorLayout {
    pub monitor_id: String,
    pub host: String,
    pub monitor_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale_factor: f64,
    pub local_x: i32,
    pub local_y: i32,
}

pub fn save_monitor_layout(layout: &MonitorLayout) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute(
        "INSERT OR REPLACE INTO monitors (monitor_id, host, monitor_name, x, y, width, height, scale_factor, local_x, local_y)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            layout.monitor_id,
            layout.host,
            layout.monitor_name,
            layout.x,
            layout.y,
            layout.width,
            layout.height,
            layout.scale_factor,
            layout.local_x,
            layout.local_y
        ],
    )?;
    // Invalidate the cache
    let mut cache = MONITOR_LAYOUT_CACHE.write().unwrap();
    *cache = None;
    Ok(())
}

pub fn get_all_monitor_layouts() -> Result<Vec<MonitorLayout>> {
    {
        let cache = MONITOR_LAYOUT_CACHE.read().unwrap();
        if let Some(ref layouts) = *cache {
            return Ok(layouts.clone());
        }
    }

    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    let mut stmt = conn.prepare("SELECT monitor_id, host, monitor_name, x, y, width, height, scale_factor, local_x, local_y FROM monitors")?;
    let iter = stmt.query_map([], |row| {
        Ok(MonitorLayout {
            monitor_id: row.get(0)?,
            host: row.get(1)?,
            monitor_name: row.get(2)?,
            x: row.get(3)?,
            y: row.get(4)?,
            width: row.get(5)?,
            height: row.get(6)?,
            scale_factor: row.get(7)?,
            local_x: row.get(8)?,
            local_y: row.get(9)?,
        })
    })?;
    let mut list = Vec::new();
    for item in iter {
        list.push(item?);
    }

    let mut cache = MONITOR_LAYOUT_CACHE.write().unwrap();
    *cache = Some(list.clone());

    Ok(list)
}

pub fn remove_monitor_layout(monitor_id: &str) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute("DELETE FROM monitors WHERE monitor_id = ?", params![monitor_id])?;
    // Invalidate the cache
    let mut cache = MONITOR_LAYOUT_CACHE.write().unwrap();
    *cache = None;
    Ok(())
}

pub fn remove_monitor_layouts_by_host(host: &str) -> Result<()> {
    let db_path = get_db_path();
    let conn = Connection::open(&db_path)?;
    conn.execute("DELETE FROM monitors WHERE host = ?", params![host])?;
    // Invalidate the cache
    let mut cache = MONITOR_LAYOUT_CACHE.write().unwrap();
    *cache = None;
    Ok(())
}
