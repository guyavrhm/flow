use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::config::SettingsData;
use crate::network::tcp::{TcpServer, TcpClient};
use crate::network::protocol::ClipboardPayload;
use crate::hardware::{ClipboardController, ClipboardListener};
use crate::engine::{OfferedData, ClipboardAccumulator};

pub struct ClipboardSyncManager {
    settings: Arc<Mutex<SettingsData>>,
    tcp_server: Arc<TcpServer>,
    tcp_client: Arc<TcpClient>,
    offered_data: Arc<Mutex<Option<OfferedData>>>,
    clipboard_listener: Arc<Mutex<Option<ClipboardListener>>>,
}

impl ClipboardSyncManager {
    pub fn new(
        settings: Arc<Mutex<SettingsData>>,
        tcp_server: Arc<TcpServer>,
        tcp_client: Arc<TcpClient>,
        offered_data: Arc<Mutex<Option<OfferedData>>>,
        clipboard_listener: Arc<Mutex<Option<ClipboardListener>>>,
    ) -> Self {
        Self {
            settings,
            tcp_server,
            tcp_client,
            offered_data,
            clipboard_listener,
        }
    }

    pub fn start(&self) {
        let settings = {
            let s = self.settings.lock().unwrap();
            s.clone()
        };
        let tcp_server = self.tcp_server.clone();
        let tcp_client = self.tcp_client.clone();
        let offered_data_mon = self.offered_data.clone();

        let on_change = move || {
            let data = ClipboardController::data();
            if data.is_empty() || data == "unknown format" {
                return;
            }

            let mut is_files = false;
            let mut total_size = 0;
            if data.starts_with("file://") || data.starts_with('/') {
                is_files = true;
                for path_str in data.lines() {
                    let path_str = path_str.trim_start_matches("file://");
                    let path = std::path::Path::new(path_str);
                    if path.exists() {
                        if path.is_file() {
                            if let Ok(metadata) = std::fs::metadata(path) {
                                total_size += metadata.len() as usize;
                            }
                        } else if path.is_dir() {
                            for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                                if entry.path().is_file() {
                                    if let Ok(metadata) = std::fs::metadata(entry.path()) {
                                        total_size += metadata.len() as usize;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                total_size = data.len();
            }

            let data_hash = hash_clipboard_data(&data);
            let is_from_network = crate::hardware::check_and_consume_ignore_hash(data_hash);

            if !is_from_network {
                if total_size > 5 * 1024 * 1024 {
                    let id = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        .to_string();
                    let format = if is_files { "files".to_string() } else { "text".to_string() };
                    log::info!("Clipboard: Local content is large ({} bytes). Registering promise id: {}", total_size, id);

                    {
                        let mut offered = offered_data_mon.lock().unwrap();
                        *offered = Some(OfferedData {
                            id: id.clone(),
                            format: format.clone(),
                            data: data.clone(),
                        });
                    }

                    let payload = ClipboardPayload::Offer { id, size: total_size, format };
                    if settings.pc == 1 {
                        tcp_server.broadcast_clipboard(&payload, None);
                    } else {
                        let _ = tcp_client.send_clipboard(&payload);
                    }
                } else {
                    if let Some(payload) = format_clipboard_data(&data) {
                        log::info!("Clipboard: Broadcasting local small clipboard update ({} bytes)", total_size);
                        if settings.pc == 1 {
                            tcp_server.broadcast_clipboard(&payload, None);
                        } else {
                            let _ = tcp_client.send_clipboard(&payload);
                        }
                    }
                }
            }
        };

        let listener = ClipboardListener::new(on_change);
        listener.start();
        {
            let mut cl = self.clipboard_listener.lock().unwrap();
            *cl = Some(listener);
        }
    }
}

pub fn hash_clipboard_data(data: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    data.hash(&mut hasher);
    hasher.finish()
}

pub fn format_clipboard_data(paths_str: &str) -> Option<ClipboardPayload> {
    let paths: Vec<&str> = paths_str.lines().filter(|line| !line.is_empty()).collect();
    if paths.is_empty() {
        return None;
    }

    let mut files = Vec::new();
    let max_file_size = 50 * 1024 * 1024; // 50 MB

    let first_path = std::path::Path::new(paths[0]);
    let root_dir = match first_path.parent() {
        Some(p) => p,
        None => return None,
    };

    for path_str in paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            log::warn!("Clipboard: Path does not exist: {:?}", path);
            continue;
        }

        if path.is_file() {
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() > max_file_size {
                    log::warn!("Clipboard: Skipping {:?} (size {} bytes exceeds max limit of 50 MB)", path, metadata.len());
                    continue;
                }
            }
            match std::fs::read(path) {
                Ok(data) => {
                    if let Ok(rel) = path.strip_prefix(root_dir) {
                        files.push(crate::network::protocol::ClipboardFile {
                            is_dir: false,
                            name: rel.to_string_lossy().to_string().replace('\\', "/"),
                            data: Some(data),
                        });
                    }
                }
                Err(e) => {
                    log::error!("Clipboard: Failed to read file {:?}: {:?}", path, e);
                }
            }
        } else if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(root_dir) {
                files.push(crate::network::protocol::ClipboardFile {
                    is_dir: true,
                    name: rel.to_string_lossy().to_string().replace('\\', "/"),
                    data: None,
                });
            }

            for entry in walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let entry_path = entry.path();
                if entry_path == path {
                    continue;
                }
                if let Ok(rel) = entry_path.strip_prefix(root_dir) {
                    let rel_name = rel.to_string_lossy().to_string().replace('\\', "/");
                    if entry_path.is_file() {
                        if let Ok(metadata) = std::fs::metadata(entry_path) {
                            if metadata.len() > max_file_size {
                                log::warn!("Clipboard: Skipping nested file {:?} (size {} bytes exceeds max limit of 50 MB)", entry_path, metadata.len());
                                continue;
                            }
                        }
                        match std::fs::read(entry_path) {
                            Ok(data) => {
                                files.push(crate::network::protocol::ClipboardFile {
                                    is_dir: false,
                                    name: rel_name,
                                    data: Some(data),
                                });
                            }
                            Err(e) => {
                                log::error!("Clipboard: Failed to read nested file {:?}: {:?}", entry_path, e);
                            }
                        }
                    } else if entry_path.is_dir() {
                        files.push(crate::network::protocol::ClipboardFile {
                            is_dir: true,
                            name: rel_name,
                            data: None,
                        });
                    }
                }
            }
        }
    }

    if files.is_empty() {
        None
    } else {
        log::debug!("Clipboard: Formatted {} clipboard files to payload", files.len());
        Some(ClipboardPayload::Files { files })
    }
}

pub fn stream_offered_data<F>(id: &str, format: &str, data: &str, mut send_chunk: F)
where
    F: FnMut(ClipboardPayload),
{
    log::info!("Streaming clipboard data for promise id: {}, format: {}", id, format);
    let bytes = if format == "text" {
        data.as_bytes().to_vec()
    } else {
        if let Some(ClipboardPayload::Files { files }) = format_clipboard_data(data) {
            match serde_json::to_vec(&files) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Failed to serialize files payload for promise streaming: {:?}", e);
                    return;
                }
            }
        } else {
            log::warn!("No files to stream for format files, data: {}", data);
            return;
        }
    };

    let total_len = bytes.len();
    let chunk_size = 64 * 1024; // 64 KB
    let mut chunk_index = 0;
    let mut offset = 0;

    while offset < total_len {
        let end = std::cmp::min(offset + chunk_size, total_len);
        let chunk_data = bytes[offset..end].to_vec();
        offset = end;
        let is_last = offset >= total_len;

        let payload = ClipboardPayload::Chunk {
            id: id.to_string(),
            chunk_index,
            is_last,
            data: chunk_data,
        };

        send_chunk(payload);
        chunk_index += 1;

        thread::sleep(Duration::from_millis(5));
    }
}

pub fn handle_incoming_clipboard_chunk(
    id: &str,
    is_last: bool,
    data: &[u8],
    accumulator: &Arc<Mutex<Option<ClipboardAccumulator>>>,
) {
    let has_tx = {
        let active_lock = crate::hardware::ACTIVE_PROMISE.lock().unwrap();
        active_lock.as_ref()
            .filter(|p| p.uuid == id)
            .map(|p| p.tx.is_some())
            .unwrap_or(false)
    };

    if !has_tx {
        return;
    }

    let mut accum_lock = accumulator.lock().unwrap();

    if accum_lock.is_none() || accum_lock.as_ref().map(|a| &a.id) != Some(&id.to_string()) {
        let format = crate::hardware::get_active_promise_format(id).unwrap_or_else(|| "text".to_string());
        let size = crate::hardware::ACTIVE_PROMISE.lock().unwrap().as_ref()
            .filter(|p| p.uuid == id)
            .map(|p| p.size)
            .unwrap_or(0);

        *accum_lock = Some(ClipboardAccumulator {
            id: id.to_string(),
            format,
            size,
            buffer: Vec::new(),
        });

        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(1, std::sync::atomic::Ordering::Relaxed);
    }

    let mut is_done = false;
    let mut format = String::new();
    let mut buffer = Vec::new();

    if let Some(ref mut accum) = *accum_lock {
        accum.buffer.extend_from_slice(data);
        if accum.size > 0 {
            let percent = (accum.buffer.len() * 100) / accum.size;
            let percent = std::cmp::min(100, percent as u32);
            crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(percent, std::sync::atomic::Ordering::Relaxed);
        }
        if is_last {
            is_done = true;
            format = accum.format.clone();
            buffer = std::mem::take(&mut accum.buffer);
        }
    }

    if is_done {
        *accum_lock = None;
        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);

        let payload = match format.as_str() {
            "text" => {
                if let Ok(text) = String::from_utf8(buffer) {
                    Some(crate::hardware::FulfillmentPayload::Text(text))
                } else {
                    log::error!("Clipboard: Failed to decode text payload UTF-8 string");
                    None
                }
            }
            "files" => {
                if let Ok(files) = serde_json::from_slice::<Vec<crate::network::protocol::ClipboardFile>>(&buffer) {
                    let local_paths = crate::network::protocol::write_clipboard_files(&files);
                    Some(crate::hardware::FulfillmentPayload::Files(local_paths))
                } else {
                    log::error!("Clipboard: Failed to deserialize files payload from JSON");
                    None
                }
            }
            _ => None,
        };

        if let Some(p) = payload {
            crate::hardware::PromisedClipboard::fulfill_promise(id, p);
        }
    }
}
