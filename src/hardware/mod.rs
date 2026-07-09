#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "macos")]
pub use mac::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
    init_keyboard_layout,
};

#[cfg(target_os = "windows")]
pub mod win;

#[cfg(target_os = "windows")]
pub use win::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
};

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
};

#[cfg(not(target_os = "macos"))]
pub fn init_keyboard_layout() {}

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

