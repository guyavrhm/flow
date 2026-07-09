// Windows stubs (placeholder)
#![allow(dead_code)]

pub struct MouseController;
impl MouseController {
    pub fn new() -> Self {
        Self
    }
    pub fn position(&self) -> (i32, i32) {
        (0, 0)
    }
    pub fn set_position(&self, _pos: (i32, i32)) {}
    pub fn press(&self, _button: &str) {}
    pub fn release(&self, _button: &str) {}
    pub fn scroll(&self, _dx: i32, _dy: i32) {}
}

pub struct MouseListener;
impl MouseListener {
    pub fn new<M, C, S>(_on_move: M, _on_click: C, _on_scroll: S, _suppress: bool) -> Self
    where
        M: Fn(i32, i32) + Send + 'static,
        C: Fn(i32, i32, String, bool) + Send + 'static,
        S: Fn(i32, i32, i32, i32) + Send + 'static,
    {
        Self
    }
    pub fn start(&self) {}
    pub fn stop(&self) {}
    pub fn join(&self) {}
}

pub struct KeyboardController;
impl KeyboardController {
    pub fn new() -> Self {
        Self
    }
    pub fn press(&self, _key: &str) {}
    pub fn release(&self, _key: &str) {}
}

pub struct KeyboardListener;
impl KeyboardListener {
    pub fn new<P, R>(_on_press: P, _on_release: R, _suppress: bool) -> Self
    where
        P: Fn(String) + Send + 'static,
        R: Fn(String) + Send + 'static,
    {
        Self
    }
    pub fn start(&self) {}
    pub fn stop(&self) {}
    pub fn join(&self) {}
}

pub struct Clipboard;
impl Clipboard {
    pub fn data() -> String {
        String::new()
    }
    pub fn set_text(_text: &str) {}
    pub fn set_files(_files: Vec<String>) {}
}

pub fn get_screeninfo() -> (i32, i32) {
    (1920, 1080)
}
