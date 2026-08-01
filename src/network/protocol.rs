use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MonitorInfo {
    pub name: String,
    pub local_x: i32,
    pub local_y: i32,
    pub width: i32,
    pub height: i32,
    pub scale_factor: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScreenMetrics {
    pub width: i32,
    pub height: i32,
    pub monitors: Vec<MonitorInfo>,
    pub uses_physical_pixels: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UdpSessionConfig {
    pub key: [u8; 32],
    pub salt: [u8; 4],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipboardFile {
    pub is_dir: bool,
    pub name: String, // Relative path from root
    pub data: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClipboardPayload {
    Text {
        text: String,
    },
    Files {
        files: Vec<ClipboardFile>,
    },
    Offer {
        id: String,
        size: usize,
        format: String, // "text" or "files"
    },
    Request {
        id: String,
    },
    Chunk {
        id: String,
        chunk_index: u64,
        is_last: bool,
        data: Vec<u8>,
    },
}

// UDP input event payload
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InputEvent {
    Move { x: i32, y: i32 },
    MouseClick { button: String, pressed: bool },
    MouseScroll { dx: i32, dy: i32 },
    KeyPress { key: String, pressed: bool },
    Stop,
}

pub fn recv_exactly<R: Read>(stream: &mut R, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn true_send<W: Write>(stream: &mut W, payload: &[u8]) -> std::io::Result<()> {
    let len_str = format!("{:010}", payload.len());
    stream.write_all(len_str.as_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

pub fn true_recv<R: Read>(stream: &mut R) -> std::io::Result<Vec<u8>> {
    let len_bytes = recv_exactly(stream, 10)?;
    let len_str = std::str::from_utf8(&len_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len: usize = len_str
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let data = recv_exactly(stream, len)?;
    Ok(data)
}

pub fn write_clipboard_files(files: &[ClipboardFile]) -> Vec<String> {
    let temp_dir = std::env::temp_dir().join("flow");
    log::debug!("Clipboard: Writing {} received files/directories to temporary folder {:?}", files.len(), temp_dir);
    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Err(e) = std::fs::create_dir_all(&temp_dir) {
        log::error!("Clipboard: Failed to create temp directory {:?}: {:?}", temp_dir, e);
    }

    let mut top_level_paths = Vec::new();

    for file in files {
        let mut safe_relative = std::path::PathBuf::new();
        for component in std::path::Path::new(&file.name).components() {
            match component {
                std::path::Component::Normal(c) => {
                    safe_relative.push(c);
                }
                _ => {} // Skip RootDir, Prefix, ParentDir (..), CurDir (.)
            }
        }
        if safe_relative.as_os_str().is_empty() {
            continue;
        }

        let dest_path = temp_dir.join(&safe_relative);
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if file.is_dir {
            if let Err(e) = std::fs::create_dir_all(&dest_path) {
                log::error!("Clipboard: Failed to create directory {:?}: {:?}", dest_path, e);
            }
        } else if let Some(ref data) = file.data {
            if let Err(e) = std::fs::write(&dest_path, data) {
                log::error!("Clipboard: Failed to write file {:?}: {:?}", dest_path, e);
            }
        }

        let mut components = dest_path.strip_prefix(&temp_dir).unwrap().components();
        if let Some(first_comp) = components.next() {
            let top_level = temp_dir.join(first_comp.as_os_str());
            let path_str = top_level.to_string_lossy().to_string();
            if !top_level_paths.contains(&path_str) {
                top_level_paths.push(path_str);
            }
        }
    }

    top_level_paths
}
