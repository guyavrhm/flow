use rusqlite::{Connection, Result, params};
use std::fs;
use crate::paths::get_db_path;

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
