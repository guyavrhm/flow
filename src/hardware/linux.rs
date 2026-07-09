// Linux X11 Implementation of Mouse, Keyboard and Clipboard controllers/listeners.
#![allow(dead_code)]

use std::ptr;
use std::os::raw::{c_int, c_uint, c_ulong, c_char, c_uchar, c_void};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::process::Command;
use std::time::Duration;

// --- X11 types ---
pub type Display = c_void;
pub type Window = c_ulong;
pub type Cursor = c_ulong;
pub type Time = c_ulong;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct XEvent {
    pub type_: c_int,
    pub pad: [c_ulong; 24],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct XMotionEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut Display,
    pub window: Window,
    pub root: Window,
    pub subwindow: Window,
    pub time: Time,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub is_hint: c_char,
    pub same_screen: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct XButtonEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut Display,
    pub window: Window,
    pub root: Window,
    pub subwindow: Window,
    pub time: Time,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub button: c_uint,
    pub same_screen: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct XKeyEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: c_int,
    pub display: *mut Display,
    pub window: Window,
    pub root: Window,
    pub subwindow: Window,
    pub time: Time,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub keycode: c_uint,
    pub same_screen: c_int,
}

const KEY_PRESS: c_int = 2;
const KEY_RELEASE: c_int = 3;
const BUTTON_PRESS: c_int = 4;
const BUTTON_RELEASE: c_int = 5;
const MOTION_NOTIFY: c_int = 6;

const GRAB_MODE_ASYNC: c_int = 1;
const NONE: c_ulong = 0;
const CURRENT_TIME: c_ulong = 0;

#[link(name = "X11")]
unsafe extern "C" {
    fn XOpenDisplay(display_name: *const c_char) -> *mut Display;
    fn XCloseDisplay(display: *mut Display);
    fn XDefaultRootWindow(display: *mut Display) -> Window;
    fn XDefaultScreen(display: *mut Display) -> c_int;
    fn XDisplayWidth(display: *mut Display, screen_number: c_int) -> c_int;
    fn XDisplayHeight(display: *mut Display, screen_number: c_int) -> c_int;
    fn XFlush(display: *mut Display);
    fn XQueryPointer(
        display: *mut Display,
        w: Window,
        root_return: *mut Window,
        child_return: *mut Window,
        root_x_return: *mut c_int,
        root_y_return: *mut c_int,
        win_x_return: *mut c_int,
        win_y_return: *mut c_int,
        mask_return: *mut c_uint,
    ) -> c_int;
    fn XWarpPointer(
        display: *mut Display,
        src_w: Window,
        dest_w: Window,
        src_x: c_int,
        src_y: c_int,
        src_width: c_uint,
        src_height: c_uint,
        dest_x: c_int,
        dest_y: c_int,
    ) -> c_int;
    fn XGrabPointer(
        display: *mut Display,
        grab_window: Window,
        owner_events: c_int,
        event_mask: c_uint,
        pointer_mode: c_int,
        keyboard_mode: c_int,
        confine_to: Window,
        cursor: Cursor,
        time: Time,
    ) -> c_int;
    fn XUngrabPointer(display: *mut Display, time: Time) -> c_int;
    fn XGrabKeyboard(
        display: *mut Display,
        grab_window: Window,
        owner_events: c_int,
        pointer_mode: c_int,
        keyboard_mode: c_int,
        time: Time,
    ) -> c_int;
    fn XUngrabKeyboard(display: *mut Display, time: Time) -> c_int;
    fn XPending(display: *mut Display) -> c_int;
    fn XNextEvent(display: *mut Display, event_return: *mut XEvent) -> c_int;
    fn XKeysymToKeycode(display: *mut Display, keysym: c_ulong) -> c_uchar;
    fn XKeycodeToKeysym(display: *mut Display, keycode: c_uchar, index: c_int) -> c_ulong;
    fn XCreateSimpleWindow(
        display: *mut Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> Window;
    fn XDestroyWindow(display: *mut Display, w: Window) -> c_int;
}

#[link(name = "Xtst")]
unsafe extern "C" {
    fn XTestFakeButtonEvent(
        display: *mut Display,
        button: c_uint,
        is_press: c_int,
        delay: c_ulong,
    ) -> c_int;
    fn XTestFakeKeyEvent(
        display: *mut Display,
        keycode: c_uint,
        is_press: c_int,
        delay: c_ulong,
    ) -> c_int;
}

// Helper to get X11 keycode from key name
fn get_linux_keycode(display: *mut Display, key: &str) -> Option<u8> {
    let keysym = if key.starts_with("Key.") {
        match key {
            "Key.backspace" => 0xFF08,
            "Key.tab" => 0xFF09,
            "Key.enter" => 0xFF0D,
            "Key.esc" | "Key.escape" => 0xFF1B,
            "Key.space" => 0x0020,
            "Key.home" => 0xFF50,
            "Key.end" => 0xFF57,
            "Key.page_up" => 0xFF55,
            "Key.page_down" => 0xFF56,
            "Key.delete" => 0xFFFF,
            "Key.left" => 0xFF51,
            "Key.right" => 0xFF52,
            "Key.down" => 0xFF54,
            "Key.up" => 0xFF53,
            "Key.cmd" | "Key.cmd_l" => 0xFFEB, // Super_L
            "Key.cmd_r" => 0xFFEC, // Super_R
            "Key.shift" | "Key.shift_l" => 0xFFE1,
            "Key.shift_r" => 0xFFE2,
            "Key.ctrl" | "Key.ctrl_l" => 0xFFE3,
            "Key.ctrl_r" => 0xFFE4,
            "Key.alt" | "Key.alt_l" => 0xFFE9,
            "Key.alt_r" => 0xFFEA,
            "Key.caps_lock" => 0xFFE5,
            "Key.f1" => 0xFFBE,
            "Key.f2" => 0xFFBF,
            "Key.f3" => 0xFFC0,
            "Key.f4" => 0xFFC1,
            "Key.f5" => 0xFFC2,
            "Key.f6" => 0xFFC3,
            "Key.f7" => 0xFFC4,
            "Key.f8" => 0xFFC5,
            "Key.f9" => 0xFFC6,
            "Key.f10" => 0xFFC7,
            "Key.f11" => 0xFFC8,
            "Key.f12" => 0xFFC9,
            _ => 0,
        }
    } else {
        let mut k = key;
        if k.len() >= 3 && k.starts_with('\'') && k.ends_with('\'') {
            k = &k[1..k.len() - 1];
        }
        if k.is_empty() {
            return None;
        }
        let first_char = k.chars().next()?;
        first_char as c_ulong
    };

    if keysym == 0 {
        return None;
    }

    unsafe {
        let code = XKeysymToKeycode(display, keysym);
        if code == 0 {
            None
        } else {
            Some(code)
        }
    }
}

// Helper to get key name from keysym
pub fn keysym_to_key(keysym: c_ulong) -> Option<String> {
    let key_str = match keysym {
        0xFF08 => "Key.backspace",
        0xFF09 => "Key.tab",
        0xFF0D => "Key.enter",
        0xFF1B => "Key.esc",
        0x0020 => "Key.space",
        0xFF50 => "Key.home",
        0xFF57 => "Key.end",
        0xFF55 => "Key.page_up",
        0xFF56 => "Key.page_down",
        0xFFFF => "Key.delete",
        0xFF51 => "Key.left",
        0xFF52 => "Key.right",
        0xFF54 => "Key.down",
        0xFF53 => "Key.up",
        0xFFEB => "Key.cmd",
        0xFFEC => "Key.cmd_r",
        0xFFE1 => "Key.shift",
        0xFFE2 => "Key.shift_r",
        0xFFE3 => "Key.ctrl",
        0xFFE4 => "Key.ctrl_r",
        0xFFE9 => "Key.alt",
        0xFFEA => "Key.alt_r",
        0xFFE5 => "Key.caps_lock",
        0xFFBE => "Key.f1",
        0xFFBF => "Key.f2",
        0xFFC0 => "Key.f3",
        0xFFC1 => "Key.f4",
        0xFFC2 => "Key.f5",
        0xFFC3 => "Key.f6",
        0xFFC4 => "Key.f7",
        0xFFC5 => "Key.f8",
        0xFFC6 => "Key.f9",
        0xFFC7 => "Key.f10",
        0xFFC8 => "Key.f11",
        0xFFC9 => "Key.f12",
        val if val >= 0x20 && val <= 0x7E => {
            let c = val as u8 as char;
            return Some(format!("'{}'", c));
        }
        _ => return None,
    };
    Some(key_str.to_string())
}

// --- MouseController ---
pub struct MouseController;

impl MouseController {
    pub fn new() -> Self {
        Self
    }

    pub fn position(&self) -> (i32, i32) {
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return (0, 0);
            }
            let root = XDefaultRootWindow(display);
            let mut root_return = 0;
            let mut child_return = 0;
            let mut root_x = 0;
            let mut root_y = 0;
            let mut win_x = 0;
            let mut win_y = 0;
            let mut mask = 0;
            XQueryPointer(
                display,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            );
            XCloseDisplay(display);
            (root_x as i32, root_y as i32)
        }
    }

    pub fn set_position(&self, pos: (i32, i32)) {
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            let root = XDefaultRootWindow(display);
            XWarpPointer(display, 0, root, 0, 0, 0, 0, pos.0 as c_int, pos.1 as c_int);
            XFlush(display);
            XCloseDisplay(display);
        }
    }

    pub fn press(&self, button: &str) {
        let btn = match button.to_lowercase().as_str() {
            "left" | "button.left" => 1,
            "middle" | "button.middle" => 2,
            "right" | "button.right" => 3,
            "x1" | "button8" => 8,
            "x2" | "button9" => 9,
            _ => return,
        };
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            XTestFakeButtonEvent(display, btn, 1, 0);
            XFlush(display);
            XCloseDisplay(display);
        }
    }

    pub fn release(&self, button: &str) {
        let btn = match button.to_lowercase().as_str() {
            "left" | "button.left" => 1,
            "middle" | "button.middle" => 2,
            "right" | "button.right" => 3,
            "x1" | "button8" => 8,
            "x2" | "button9" => 9,
            _ => return,
        };
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            XTestFakeButtonEvent(display, btn, 0, 0);
            XFlush(display);
            XCloseDisplay(display);
        }
    }

    pub fn scroll(&self, dx: i32, dy: i32) {
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            if dy != 0 {
                let btn = if dy > 0 { 4 } else { 5 };
                for _ in 0..dy.abs() {
                    XTestFakeButtonEvent(display, btn, 1, 0);
                    XTestFakeButtonEvent(display, btn, 0, 0);
                }
            }
            if dx != 0 {
                let btn = if dx > 0 { 7 } else { 6 };
                for _ in 0..dx.abs() {
                    XTestFakeButtonEvent(display, btn, 1, 0);
                    XTestFakeButtonEvent(display, btn, 0, 0);
                }
            }
            XFlush(display);
            XCloseDisplay(display);
        }
    }
}

// --- MouseListener ---
pub struct MouseListener {
    running: Arc<AtomicBool>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    on_move: Arc<dyn Fn(i32, i32) + Send + Sync + 'static>,
    on_click: Arc<dyn Fn(i32, i32, String, bool) + Send + Sync + 'static>,
    on_scroll: Arc<dyn Fn(i32, i32, i32, i32) + Send + Sync + 'static>,
    suppress: bool,
}

impl MouseListener {
    pub fn new<M, C, S>(on_move: M, on_click: C, on_scroll: S, suppress: bool) -> Self
    where
        M: Fn(i32, i32) + Send + Sync + 'static,
        C: Fn(i32, i32, String, bool) + Send + Sync + 'static,
        S: Fn(i32, i32, i32, i32) + Send + Sync + 'static,
    {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread: Arc::new(Mutex::new(None)),
            on_move: Arc::new(on_move),
            on_click: Arc::new(on_click),
            on_scroll: Arc::new(on_scroll),
            suppress,
        }
    }

    pub fn start(&self) {
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);
        let on_move = self.on_move.clone();
        let on_click = self.on_click.clone();
        let on_scroll = self.on_scroll.clone();
        let suppress = self.suppress;

        let handle = thread::spawn(move || {
            unsafe {
                let display = XOpenDisplay(ptr::null());
                if display.is_null() {
                    return;
                }
                let root = XDefaultRootWindow(display);
                let window = XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0);

                let event_mask = 4 | 8 | 64; // ButtonPressMask | ButtonReleaseMask | PointerMotionMask

                if suppress {
                    XGrabPointer(
                        display,
                        window,
                        0,
                        event_mask as c_uint,
                        GRAB_MODE_ASYNC,
                        GRAB_MODE_ASYNC,
                        NONE,
                        NONE,
                        CURRENT_TIME,
                    );
                }

                let (width, height) = get_screeninfo();
                let x_center = width / 2;
                let y_center = height / 2;

                if suppress {
                    XWarpPointer(display, 0, root, 0, 0, 0, 0, x_center, y_center);
                    XFlush(display);
                }

                while running.load(Ordering::SeqCst) {
                    if XPending(display) > 0 {
                        let mut event: XEvent = std::mem::zeroed();
                        XNextEvent(display, &mut event);

                        match event.type_ {
                            MOTION_NOTIFY => {
                                let motion_ev = &*( &event as *const XEvent as *const XMotionEvent );
                                if motion_ev.x_root == x_center && motion_ev.y_root == y_center {
                                    continue;
                                }
                                let dx = motion_ev.x_root - x_center;
                                let dy = motion_ev.y_root - y_center;
                                if dx != 0 || dy != 0 {
                                    (on_move)(dx, dy);
                                    if suppress {
                                        XWarpPointer(display, 0, root, 0, 0, 0, 0, x_center, y_center);
                                        XFlush(display);
                                    }
                                }
                            }
                            BUTTON_PRESS | BUTTON_RELEASE => {
                                let button_ev = &*( &event as *const XEvent as *const XButtonEvent );
                                let is_press = event.type_ == BUTTON_PRESS;
                                let btn = button_ev.button;
                                if btn == 4 || btn == 5 || btn == 6 || btn == 7 {
                                    if is_press {
                                        let dy = if btn == 4 { 1 } else if btn == 5 { -1 } else { 0 };
                                        let dx = if btn == 7 { 1 } else if btn == 6 { -1 } else { 0 };
                                        (on_scroll)(button_ev.x_root, button_ev.y_root, dx, dy);
                                    }
                                } else {
                                    let btn_name = match btn {
                                        1 => "Button.left",
                                        2 => "Button.middle",
                                        3 => "Button.right",
                                        8 => "Button.x1",
                                        9 => "Button.x2",
                                        _ => "Button.unknown",
                                    };
                                    (on_click)(button_ev.x_root, button_ev.y_root, btn_name.to_string(), is_press);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        thread::sleep(Duration::from_millis(5));
                    }
                }

                if suppress {
                    XUngrabPointer(display, CURRENT_TIME);
                }
                XDestroyWindow(display, window);
                XCloseDisplay(display);
            }
        });

        *self.thread.lock().unwrap() = Some(handle);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn join(&self) {
        let handle = self.thread.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for MouseListener {
    fn drop(&mut self) {
        self.stop();
        self.join();
    }
}

// --- KeyboardController ---
pub struct KeyboardController;

impl KeyboardController {
    pub fn new() -> Self {
        Self
    }

    pub fn press(&self, key: &str) {
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            if let Some(code) = get_linux_keycode(display, key) {
                XTestFakeKeyEvent(display, code as c_uint, 1, 0);
                XFlush(display);
            }
            XCloseDisplay(display);
        }
    }

    pub fn release(&self, key: &str) {
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return;
            }
            if let Some(code) = get_linux_keycode(display, key) {
                XTestFakeKeyEvent(display, code as c_uint, 0, 0);
                XFlush(display);
            }
            XCloseDisplay(display);
        }
    }
}

// --- KeyboardListener ---
pub struct KeyboardListener {
    running: Arc<AtomicBool>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    on_press: Arc<dyn Fn(String) + Send + Sync + 'static>,
    on_release: Arc<dyn Fn(String) + Send + Sync + 'static>,
    suppress: bool,
}

impl KeyboardListener {
    pub fn new<P, R>(on_press: P, on_release: R, suppress: bool) -> Self
    where
        P: Fn(String) + Send + Sync + 'static,
        R: Fn(String) + Send + Sync + 'static,
    {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread: Arc::new(Mutex::new(None)),
            on_press: Arc::new(on_press),
            on_release: Arc::new(on_release),
            suppress,
        }
    }

    pub fn start(&self) {
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);
        let on_press = self.on_press.clone();
        let on_release = self.on_release.clone();
        let suppress = self.suppress;

        let handle = thread::spawn(move || {
            unsafe {
                let display = XOpenDisplay(ptr::null());
                if display.is_null() {
                    return;
                }
                let root = XDefaultRootWindow(display);
                let window = XCreateSimpleWindow(display, root, 0, 0, 1, 1, 0, 0, 0);

                if suppress {
                    XGrabKeyboard(
                        display,
                        window,
                        0,
                        GRAB_MODE_ASYNC,
                        GRAB_MODE_ASYNC,
                        CURRENT_TIME,
                    );
                }

                while running.load(Ordering::SeqCst) {
                    if XPending(display) > 0 {
                        let mut event: XEvent = std::mem::zeroed();
                        XNextEvent(display, &mut event);

                        match event.type_ {
                            KEY_PRESS | KEY_RELEASE => {
                                let key_ev = &*( &event as *const XEvent as *const XKeyEvent );
                                let is_press = event.type_ == KEY_PRESS;
                                let keysym = XKeycodeToKeysym(display, key_ev.keycode as u8, 0);
                                if let Some(key_name) = keysym_to_key(keysym) {
                                    if is_press {
                                        (on_press)(key_name);
                                    } else {
                                        (on_release)(key_name);
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else {
                        thread::sleep(Duration::from_millis(5));
                    }
                }

                if suppress {
                    XUngrabKeyboard(display, CURRENT_TIME);
                }
                XDestroyWindow(display, window);
                XCloseDisplay(display);
            }
        });

        *self.thread.lock().unwrap() = Some(handle);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn join(&self) {
        let handle = self.thread.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for KeyboardListener {
    fn drop(&mut self) {
        self.stop();
        self.join();
    }
}

// --- Clipboard ---
pub struct Clipboard;

impl Clipboard {
    pub fn data() -> String {
        // Try xclip for files (text/uri-list) first
        if let Ok(output) = Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .arg("-o")
            .arg("-t")
            .arg("text/uri-list")
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let paths: Vec<String> = text
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.starts_with("file://") {
                            let mut path = line.trim_start_matches("file://").to_string();
                            path = url_decode(&path);
                            Some(path)
                        } else {
                            None
                        }
                    })
                    .collect();
                if !paths.is_empty() {
                    return paths.join("\n");
                }
            }
        }

        if let Ok(mut ctx) = arboard::Clipboard::new() {
            if let Ok(text) = ctx.get_text() {
                return text;
            }
        }

        String::new()
    }

    pub fn set_text(text: &str) {
        if let Ok(mut ctx) = arboard::Clipboard::new() {
            let _ = ctx.set_text(text.to_string());
        }
    }

    pub fn set_files(files: Vec<String>) {
        if files.is_empty() {
            return;
        }
        let uris: Vec<String> = files
            .into_iter()
            .map(|f| format!("file://{}", f))
            .collect();
        let payload = uris.join("\r\n") + "\r\n";

        // Write text/uri-list to xclip
        use std::io::Write;
        if let Ok(mut child) = Command::new("xclip")
            .arg("-selection")
            .arg("clipboard")
            .arg("-t")
            .arg("text/uri-list")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(payload.as_bytes());
            }
            let _ = child.wait();
        }
    }
}

fn url_decode(s: &str) -> String {
    let mut res = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if let Ok(val) = u8::from_str_radix(&hex, 16) {
                res.push(val as char);
            } else {
                res.push('%');
                res.push_str(&hex);
            }
        } else {
            res.push(c);
        }
    }
    res
}

pub fn get_screeninfo() -> (i32, i32) {
    unsafe {
        let display = XOpenDisplay(ptr::null());
        if display.is_null() {
            return (1920, 1080);
        }
        let screen = XDefaultScreen(display);
        let width = XDisplayWidth(display, screen);
        let height = XDisplayHeight(display, screen);
        XCloseDisplay(display);
        (width as i32, height as i32)
    }
}
