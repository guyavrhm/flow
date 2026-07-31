#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "linux")]
pub mod linux;

use std::sync::atomic::AtomicU32;
use once_cell::sync::Lazy;

pub static CLIPBOARD_SYNC_PROGRESS: Lazy<std::sync::Arc<AtomicU32>> = Lazy::new(|| std::sync::Arc::new(AtomicU32::new(0)));
pub static CLIPBOARD_IGNORE_HASHES: Lazy<std::sync::Mutex<Vec<u64>>> = Lazy::new(|| std::sync::Mutex::new(std::vec::Vec::new()));
pub static IN_SET_CLIPBOARD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn push_ignore_hash(hash: u64) {
    let mut ignore_queue = CLIPBOARD_IGNORE_HASHES.lock().unwrap();
    ignore_queue.push(hash);
    if ignore_queue.len() > 10 {
        ignore_queue.remove(0);
    }
}

pub fn check_and_consume_ignore_hash(hash: u64) -> bool {
    let mut ignore_queue = CLIPBOARD_IGNORE_HASHES.lock().unwrap();
    if ignore_queue.contains(&hash) {
        ignore_queue.retain(|h| h != &hash);
        true
    } else {
        false
    }
}

#[derive(Clone, Debug)]
pub enum FulfillmentPayload {
    Text(String),
    Files(Vec<String>),
}

pub struct PromiseContext {
    pub uuid: String,
    pub format: String,
    pub size: usize,
    pub tx: Option<std::sync::mpsc::Sender<FulfillmentPayload>>,
}

pub static ACTIVE_PROMISE: Lazy<std::sync::Mutex<Option<PromiseContext>>> = Lazy::new(|| std::sync::Mutex::new(None));
type PromiseCallback = Box<dyn Fn(String) + Send + Sync + 'static>;
pub static PROMISE_REQUEST_CALLBACK: Lazy<std::sync::Mutex<Option<PromiseCallback>>> = Lazy::new(|| std::sync::Mutex::new(None));

pub fn get_active_promise_format(id: &str) -> Option<String> {
    let lock = ACTIVE_PROMISE.lock().unwrap();
    if let Some(ref active) = *lock {
        if active.uuid == id {
            return Some(active.format.clone());
        }
    }
    None
}

pub struct PromisedClipboard;

impl PromisedClipboard {
    pub fn set_promise(id: &str, format: &str, size: usize) {
        #[cfg(target_os = "macos")]
        mac::set_promise_impl(id, format, size);

        #[cfg(target_os = "windows")]
        win::set_promise_impl(id, format, size);

        #[cfg(target_os = "linux")]
        linux::set_promise_impl(id, format, size);
    }

    pub fn on_request<F>(callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let mut lock = PROMISE_REQUEST_CALLBACK.lock().unwrap();
        *lock = Some(Box::new(callback));
    }

    pub fn fulfill_promise(id: &str, payload: FulfillmentPayload) {
        let active_lock = ACTIVE_PROMISE.lock().unwrap();
        if let Some(ref active) = *active_lock {
            if active.uuid == id {
                if let Some(ref tx) = active.tx {
                    let _ = tx.send(payload);
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
    init_keyboard_layout, ClipboardListener,
};

#[cfg(target_os = "windows")]
pub use win::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
    init_keyboard_layout, ClipboardListener,
};

#[cfg(target_os = "linux")]
pub use linux::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
    init_keyboard_layout, ClipboardListener,
};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn get_screeninfo() -> (i32, i32) {
    (1920, 1080)
}

pub fn get_resource_path(name: &str) -> std::path::PathBuf {
    let local_path = std::path::PathBuf::from("resources").join(name);
    if local_path.exists() {
        return local_path;
    }
    if let Ok(mut exe_path) = std::env::current_exe() {
        exe_path.pop();
        let rel_path = exe_path.join("resources").join(name);
        if rel_path.exists() {
            return rel_path;
        }
    }
    std::path::PathBuf::from("resources").join(name)
}
