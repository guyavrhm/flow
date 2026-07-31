use arboard;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_ulong;
use std::process::Command;
use std::ptr;
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use crate::hardware::get_resource_path;
use objc::{msg_send, sel, sel_impl};

// --- SendRawPtr wrapper for FFI pointers to satisfy Send/Sync ---

#[derive(Clone, Copy)]
pub struct SendRawPtr(pub *mut c_void);
unsafe impl Send for SendRawPtr {}
unsafe impl Sync for SendRawPtr {}

// --- CoreGraphics / Carbon / CoreFoundation FFI Declarations ---

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

pub type CGEventRef = *mut c_void;
pub type CGEventSourceRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CGEventTapProxy = *mut c_void;

pub type CGEventTapCallBack = extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    refcon: *mut c_void,
) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        mouse_cursor_position: CGPoint,
        mouse_button: u32,
    ) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent(
        source: CGEventSourceRef,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
    ) -> CGEventRef;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        keycode: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        refcon: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: CFMachPortRef,
        index: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(obj: *mut c_void);
    fn CFDataGetBytePtr(the_data: *mut c_void) -> *const u8;
    fn CFDataGetLength(the_data: *mut c_void) -> isize;
    static kCFRunLoopDefaultMode: *const c_void;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(source: *mut c_void, property_key: *const c_void) -> *mut c_void;
    fn UCKeyTranslate(
        layout: *const c_void,
        keycode: u16,
        key_action: u16,
        modifier_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: u32,
        actual_string_length: *mut c_ulong,
        unicode_string: *mut u16,
    ) -> i32;
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
}

// Quartz Constants
pub const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
pub const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
pub const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
pub const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
pub const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
pub const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
pub const K_CG_EVENT_OTHER_MOUSE_UP: u32 = 26;
pub const K_CG_EVENT_LEFT_MOUSE_DRAGGED: u32 = 6;
pub const K_CG_EVENT_RIGHT_MOUSE_DRAGGED: u32 = 7;
pub const K_CG_EVENT_OTHER_MOUSE_DRAGGED: u32 = 27;
pub const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;

pub const K_CG_MOUSE_BUTTON_LEFT: u32 = 0;
pub const K_CG_MOUSE_BUTTON_RIGHT: u32 = 1;
pub const K_CG_MOUSE_BUTTON_CENTER: u32 = 2;

pub const K_CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;
pub const K_CG_MOUSE_EVENT_DELTA_X: u32 = 4;
pub const K_CG_MOUSE_EVENT_DELTA_Y: u32 = 5;

pub const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1: u32 = 97;
pub const K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2: u32 = 98;
pub const K_CG_SCROLL_EVENT_UNIT_LINE: u32 = 1;

pub const K_CG_HID_EVENT_TAP: u32 = 0;
pub const K_CG_SESSION_EVENT_TAP: u32 = 1;
pub const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
pub const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

pub const K_CG_EVENT_KEY_DOWN: u32 = 10;
pub const K_CG_EVENT_KEY_UP: u32 = 11;
pub const K_CG_EVENT_FLAGS_CHANGED: u32 = 12;
pub const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;

// --- Key Mapping Tables ---

static MACOS_KEY_MAP: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(51, "Key.backspace");
    m.insert(48, "Key.tab");
    m.insert(36, "Key.enter");
    m.insert(53, "Key.esc");
    m.insert(49, "Key.space");
    m.insert(115, "Key.home");
    m.insert(119, "Key.end");
    m.insert(116, "Key.page_up");
    m.insert(121, "Key.page_down");
    m.insert(117, "Key.delete");
    m.insert(123, "Key.left");
    m.insert(124, "Key.right");
    m.insert(125, "Key.down");
    m.insert(126, "Key.up");
    m.insert(55, "Key.cmd_l");
    m.insert(54, "Key.cmd_r");
    m.insert(56, "Key.shift_l");
    m.insert(60, "Key.shift_r");
    m.insert(59, "Key.ctrl_l");
    m.insert(62, "Key.ctrl_r");
    m.insert(58, "Key.alt_l");
    m.insert(61, "Key.alt_r");
    m.insert(57, "Key.caps_lock");
    m.insert(122, "Key.f1");
    m.insert(120, "Key.f2");
    m.insert(99, "Key.f3");
    m.insert(118, "Key.f4");
    m.insert(96, "Key.f5");
    m.insert(97, "Key.f6");
    m.insert(98, "Key.f7");
    m.insert(100, "Key.f8");
    m.insert(101, "Key.f9");
    m.insert(109, "Key.f10");
    m.insert(103, "Key.f11");
    m.insert(111, "Key.f12");
    m
});

static MACOS_REVERSE_KEY_MAP: Lazy<HashMap<String, u16>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for (k, v) in MACOS_KEY_MAP.iter() {
        m.insert(v.to_string(), *k);
    }
    m.insert("Key.cmd".to_string(), 55);
    m.insert("Key.shift".to_string(), 56);
    m.insert("Key.ctrl".to_string(), 59);
    m.insert("Key.alt".to_string(), 58);
    m
});

static US_LAYOUT: Lazy<HashMap<char, u16>> = Lazy::new(|| {
    let mut m = HashMap::new();
    let chars = "abcdefghijklmnopqrstuvwxyz0123456789";
    let codes = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6, 29, 18, 19, 20, 21, 23, 22, 26, 28, 25,
    ];
    for (c, code) in chars.chars().zip(codes.iter()) {
        m.insert(c, *code);
    }
    m.insert(' ', 49);
    m.insert('\n', 36);
    m.insert('\r', 36);
    m.insert('\t', 48);
    m.insert('-', 27);
    m.insert('=', 24);
    m.insert('[', 33);
    m.insert(']', 30);
    m.insert('\\', 42);
    m.insert(';', 41);
    m.insert('\'', 39);
    m.insert(',', 43);
    m.insert('.', 47);
    m.insert('/', 44);
    m.insert('`', 50);
    m
});

static MAC_CHAR_TO_KEYCODE: Lazy<HashMap<char, u16>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for code in 0..128 {
        if let Some(char_str) = keycode_to_char_mac(code, 0) {
            if char_str.chars().count() == 1 {
                if let Some(c) = char_str.chars().next() {
                    m.insert(c.to_ascii_lowercase(), code);
                }
            }
        }
    }
    m
});

static CACHED_LAYOUT_DATA: Lazy<RwLock<Option<Vec<u8>>>> = Lazy::new(|| RwLock::new(None));

pub fn init_keyboard_layout() {
    unsafe {
        let tis_source = TISCopyCurrentKeyboardInputSource();
        if !tis_source.is_null() {
            let layout_data_ptr =
                TISGetInputSourceProperty(tis_source, kTISPropertyUnicodeKeyLayoutData);
            if !layout_data_ptr.is_null() {
                let raw_layout = CFDataGetBytePtr(layout_data_ptr);
                if !raw_layout.is_null() {
                    let len = CFDataGetLength(layout_data_ptr);
                    if len > 0 {
                        let mut buf = vec![0u8; len as usize];
                        std::ptr::copy_nonoverlapping(raw_layout, buf.as_mut_ptr(), len as usize);
                        let mut cache = CACHED_LAYOUT_DATA.write().unwrap();
                        *cache = Some(buf);
                    }
                }
            }
            CFRelease(tis_source);
        }
    }
}

fn keycode_to_char_mac(keycode: u16, modifier_state: u32) -> Option<String> {
    let cached = {
        let cache_lock = CACHED_LAYOUT_DATA.read().unwrap();
        cache_lock.clone()
    };

    let layout_bytes = match cached {
        Some(bytes) => bytes,
        None => {
            init_keyboard_layout();
            let cache_lock = CACHED_LAYOUT_DATA.read().unwrap();
            match &*cache_lock {
                Some(bytes) => bytes.clone(),
                None => return None,
            }
        }
    };

    unsafe {
        let mut dead_keys: u32 = 0;
        let mut actual_len: c_ulong = 0;
        let mut unicode_str = [0u16; 10];

        let status = UCKeyTranslate(
            layout_bytes.as_ptr() as *const c_void,
            keycode,
            0, // Down action
            modifier_state,
            0,
            0,
            &mut dead_keys,
            10,
            &mut actual_len,
            unicode_str.as_mut_ptr(),
        );

        if status == 0 && actual_len > 0 {
            Some(String::from_utf16_lossy(
                &unicode_str[0..(actual_len as usize)],
            ))
        } else {
            None
        }
    }
}

pub fn get_mac_keycode(key: &str) -> Option<u16> {
    if key.starts_with("Key.") {
        return MACOS_REVERSE_KEY_MAP.get(key).copied();
    }

    let mut k = key;
    if k.len() >= 3 && k.starts_with('\'') && k.ends_with('\'') {
        k = &k[1..k.len() - 1];
    }

    let char_to_find = k.chars().next()?.to_ascii_lowercase();
    if let Some(code) = MAC_CHAR_TO_KEYCODE.get(&char_to_find) {
        Some(*code)
    } else {
        US_LAYOUT.get(&char_to_find).copied()
    }
}

pub fn get_screeninfo() -> (i32, i32) {
    unsafe {
        let display = CGMainDisplayID();
        let bounds = CGDisplayBounds(display);
        (bounds.size.width as i32, bounds.size.height as i32)
    }
}


// --- MouseController ---

pub struct MouseController {
    pressed_buttons: Mutex<HashMap<String, bool>>,
}

impl MouseController {
    pub fn new() -> Self {
        let mut pressed = HashMap::new();
        pressed.insert("left".to_string(), false);
        pressed.insert("right".to_string(), false);
        pressed.insert("middle".to_string(), false);
        pressed.insert("x1".to_string(), false);
        pressed.insert("x2".to_string(), false);

        Self {
            pressed_buttons: Mutex::new(pressed),
        }
    }

    pub fn position(&self) -> (i32, i32) {
        unsafe {
            let event = CGEventCreate(ptr::null_mut());
            let point = CGEventGetLocation(event);
            CFRelease(event);
            (point.x as i32, point.y as i32)
        }
    }

    pub fn set_position(&self, pos: (i32, i32)) {
        let (x, y) = pos;
        let cgp = CGPoint {
            x: x as f64,
            y: y as f64,
        };
        unsafe {
            CGWarpMouseCursorPosition(cgp);

            let pressed = self.pressed_buttons.lock().unwrap();
            let (event_type, btn) = if *pressed.get("left").unwrap_or(&false) {
                (K_CG_EVENT_LEFT_MOUSE_DRAGGED, K_CG_MOUSE_BUTTON_LEFT)
            } else if *pressed.get("right").unwrap_or(&false) {
                (K_CG_EVENT_RIGHT_MOUSE_DRAGGED, K_CG_MOUSE_BUTTON_RIGHT)
            } else if *pressed.get("middle").unwrap_or(&false) {
                (K_CG_EVENT_OTHER_MOUSE_DRAGGED, K_CG_MOUSE_BUTTON_CENTER)
            } else {
                (K_CG_EVENT_MOUSE_MOVED, K_CG_MOUSE_BUTTON_LEFT)
            };

            let event = CGEventCreateMouseEvent(ptr::null_mut(), event_type, cgp, btn);
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }

    pub fn press(&self, button: &str) {
        let button = button.to_lowercase();
        let pos = self.position();
        let cgp = CGPoint {
            x: pos.0 as f64,
            y: pos.1 as f64,
        };

        unsafe {
            let mut pressed = self.pressed_buttons.lock().unwrap();
            let event = if button == "left" {
                pressed.insert("left".to_string(), true);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_LEFT_MOUSE_DOWN,
                    cgp,
                    K_CG_MOUSE_BUTTON_LEFT,
                )
            } else if button == "right" {
                pressed.insert("right".to_string(), true);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_RIGHT_MOUSE_DOWN,
                    cgp,
                    K_CG_MOUSE_BUTTON_RIGHT,
                )
            } else if button == "middle" {
                pressed.insert("middle".to_string(), true);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_OTHER_MOUSE_DOWN,
                    cgp,
                    K_CG_MOUSE_BUTTON_CENTER,
                )
            } else if button == "x1" || button == "button8" {
                pressed.insert("x1".to_string(), true);
                CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_OTHER_MOUSE_DOWN, cgp, 3)
            } else if button == "x2" || button == "button9" {
                pressed.insert("x2".to_string(), true);
                CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_OTHER_MOUSE_DOWN, cgp, 4)
            } else {
                return;
            };

            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }

    pub fn release(&self, button: &str) {
        let button = button.to_lowercase();
        let pos = self.position();
        let cgp = CGPoint {
            x: pos.0 as f64,
            y: pos.1 as f64,
        };

        unsafe {
            let mut pressed = self.pressed_buttons.lock().unwrap();
            let event = if button == "left" {
                pressed.insert("left".to_string(), false);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_LEFT_MOUSE_UP,
                    cgp,
                    K_CG_MOUSE_BUTTON_LEFT,
                )
            } else if button == "right" {
                pressed.insert("right".to_string(), false);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_RIGHT_MOUSE_UP,
                    cgp,
                    K_CG_MOUSE_BUTTON_RIGHT,
                )
            } else if button == "middle" {
                pressed.insert("middle".to_string(), false);
                CGEventCreateMouseEvent(
                    ptr::null_mut(),
                    K_CG_EVENT_OTHER_MOUSE_UP,
                    cgp,
                    K_CG_MOUSE_BUTTON_CENTER,
                )
            } else if button == "x1" || button == "button8" {
                pressed.insert("x1".to_string(), false);
                CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_OTHER_MOUSE_UP, cgp, 3)
            } else if button == "x2" || button == "button9" {
                pressed.insert("x2".to_string(), false);
                CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_OTHER_MOUSE_UP, cgp, 4)
            } else {
                return;
            };

            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }

    pub fn scroll(&self, dx: i32, dy: i32) {
        unsafe {
            let event = CGEventCreateScrollWheelEvent(
                ptr::null_mut(),
                K_CG_SCROLL_EVENT_UNIT_LINE,
                2,
                dy,
                dx,
            );
            CGEventPost(K_CG_HID_EVENT_TAP, event);
            CFRelease(event);
        }
    }
}

// --- MouseListener ---

pub struct MouseListenerCallbacks {
    pub on_move: Box<dyn Fn(i32, i32) + Send>,
    pub on_click: Box<dyn Fn(i32, i32, String, bool) + Send>,
    pub on_scroll: Box<dyn Fn(i32, i32, i32, i32) + Send>,
    pub suppress: bool,
    pub x_center: i32,
    pub y_center: i32,
    pub has_move: bool,
}

pub struct MouseListener {
    runloop: Arc<Mutex<Option<SendRawPtr>>>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    callbacks_ptr: Arc<Mutex<Option<SendRawPtr>>>,
    on_move_active: bool,
}

impl MouseListener {
    pub fn new<M, C, S>(on_move: M, on_click: C, on_scroll: S, suppress: bool) -> Self
    where
        M: Fn(i32, i32) + Send + 'static,
        C: Fn(i32, i32, String, bool) + Send + 'static,
        S: Fn(i32, i32, i32, i32) + Send + 'static,
    {
        let screen = get_screeninfo();
        let x_center = screen.0 / 2;
        let y_center = screen.1 / 2;

        let callbacks = Box::into_raw(Box::new(MouseListenerCallbacks {
            on_move: Box::new(on_move),
            on_click: Box::new(on_click),
            on_scroll: Box::new(on_scroll),
            suppress,
            x_center,
            y_center,
            has_move: true,
        }));

        Self {
            runloop: Arc::new(Mutex::new(None)),
            thread: Arc::new(Mutex::new(None)),
            callbacks_ptr: Arc::new(Mutex::new(Some(SendRawPtr(callbacks as *mut c_void)))),
            on_move_active: true,
        }
    }

    pub fn start(&self) {
        let runloop_clone = self.runloop.clone();
        let callbacks_ptr_clone = self.callbacks_ptr.clone();
        let on_move_active = self.on_move_active;

        let handle = thread::spawn(move || unsafe {
            let rl = CFRunLoopGetCurrent();
            {
                let mut lock = runloop_clone.lock().unwrap();
                *lock = Some(SendRawPtr(rl));
            }

            let callbacks_raw = {
                let lock = callbacks_ptr_clone.lock().unwrap();
                lock.unwrap().0 as *mut MouseListenerCallbacks
            };

            let callbacks = &mut *callbacks_raw;

            if on_move_active {
                CGWarpMouseCursorPosition(CGPoint {
                    x: callbacks.x_center as f64,
                    y: callbacks.y_center as f64,
                });
                CGDisplayHideCursor(0);
            }

            let mask = (1 << K_CG_EVENT_MOUSE_MOVED)
                | (1 << K_CG_EVENT_LEFT_MOUSE_DOWN)
                | (1 << K_CG_EVENT_LEFT_MOUSE_UP)
                | (1 << K_CG_EVENT_RIGHT_MOUSE_DOWN)
                | (1 << K_CG_EVENT_RIGHT_MOUSE_UP)
                | (1 << K_CG_EVENT_OTHER_MOUSE_DOWN)
                | (1 << K_CG_EVENT_OTHER_MOUSE_UP)
                | (1 << K_CG_EVENT_LEFT_MOUSE_DRAGGED)
                | (1 << K_CG_EVENT_RIGHT_MOUSE_DRAGGED)
                | (1 << K_CG_EVENT_OTHER_MOUSE_DRAGGED)
                | (1 << K_CG_EVENT_SCROLL_WHEEL);

            let tap = CGEventTapCreate(
                K_CG_SESSION_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_DEFAULT,
                mask,
                mouse_tap_callback,
                callbacks_raw as *mut c_void,
            );

            if tap.is_null() {
                log::error!(
                    "Failed to create mouse event tap! Accessibility permissions are required."
                );
                return;
            }

            let source = CFMachPortCreateRunLoopSource(ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
            CGEventTapEnable(tap, true);

            CFRunLoopRun();

            CGEventTapEnable(tap, false);
            CFRelease(source);
            CFRelease(tap);

            if on_move_active {
                CGDisplayShowCursor(0);
            }
        });

        let mut thread_lock = self.thread.lock().unwrap();
        *thread_lock = Some(handle);
    }

    pub fn stop(&self) {
        {
            let rl_lock = self.runloop.lock().unwrap();
            if let Some(rl) = *rl_lock {
                unsafe {
                    CFRunLoopStop(rl.0);
                }
            }
        }

        let mut thread_lock = self.thread.lock().unwrap();
        if let Some(handle) = thread_lock.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MouseListener {
    fn drop(&mut self) {
        self.stop();
        let mut ptr_lock = self.callbacks_ptr.lock().unwrap();
        if let Some(ptr) = ptr_lock.take() {
            unsafe {
                let _ = Box::from_raw(ptr.0 as *mut MouseListenerCallbacks);
            }
        }
    }
}

pub extern "C" fn mouse_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    refcon: *mut c_void,
) -> CGEventRef {
    let callbacks = unsafe { &*(refcon as *mut MouseListenerCallbacks) };

    unsafe {
        let point = CGEventGetLocation(event);
        let x = point.x as i32;
        let y = point.y as i32;

        if event_type == K_CG_EVENT_MOUSE_MOVED
            || event_type == K_CG_EVENT_LEFT_MOUSE_DRAGGED
            || event_type == K_CG_EVENT_RIGHT_MOUSE_DRAGGED
            || event_type == K_CG_EVENT_OTHER_MOUSE_DRAGGED
        {
            let dx = CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_DELTA_X) as i32;
            let dy = CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_DELTA_Y) as i32;

            if dx != 0 || dy != 0 {
                CGWarpMouseCursorPosition(CGPoint {
                    x: callbacks.x_center as f64,
                    y: callbacks.y_center as f64,
                });
                (callbacks.on_move)(dx, dy);
            }
            return ptr::null_mut(); // Suppress
        } else if event_type == K_CG_EVENT_LEFT_MOUSE_DOWN
            || event_type == K_CG_EVENT_LEFT_MOUSE_UP
            || event_type == K_CG_EVENT_RIGHT_MOUSE_DOWN
            || event_type == K_CG_EVENT_RIGHT_MOUSE_UP
            || event_type == K_CG_EVENT_OTHER_MOUSE_DOWN
            || event_type == K_CG_EVENT_OTHER_MOUSE_UP
        {
            let pressed = event_type == K_CG_EVENT_LEFT_MOUSE_DOWN
                || event_type == K_CG_EVENT_RIGHT_MOUSE_DOWN
                || event_type == K_CG_EVENT_OTHER_MOUSE_DOWN;

            let button = if event_type == K_CG_EVENT_LEFT_MOUSE_DOWN
                || event_type == K_CG_EVENT_LEFT_MOUSE_UP
            {
                "Button.left".to_string()
            } else if event_type == K_CG_EVENT_RIGHT_MOUSE_DOWN
                || event_type == K_CG_EVENT_RIGHT_MOUSE_UP
            {
                "Button.right".to_string()
            } else {
                let btn_num = CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_BUTTON_NUMBER);
                match btn_num {
                    2 => "Button.middle".to_string(),
                    3 => "Button.x1".to_string(),
                    4 => "Button.x2".to_string(),
                    _ => format!("Button.button{}", btn_num),
                }
            };

            (callbacks.on_click)(x, y, button, pressed);
            if callbacks.suppress {
                return ptr::null_mut();
            }
        } else if event_type == K_CG_EVENT_SCROLL_WHEEL {
            let dy =
                CGEventGetIntegerValueField(event, K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_1) as i32;
            let dx =
                CGEventGetIntegerValueField(event, K_CG_SCROLL_WHEEL_EVENT_DELTA_AXIS_2) as i32;
            (callbacks.on_scroll)(x, y, dx, dy);
            if callbacks.suppress {
                return ptr::null_mut();
            }
        }
    }

    event
}

// --- KeyboardController ---

pub struct KeyboardController;

impl KeyboardController {
    pub fn new() -> Self {
        Self
    }

    pub fn press(&self, key: &str) {
        if let Some(code) = get_mac_keycode(key) {
            unsafe {
                let event = CGEventCreateKeyboardEvent(ptr::null_mut(), code, true);
                CGEventPost(K_CG_HID_EVENT_TAP, event);
                CFRelease(event);
            }
        }
    }

    pub fn release(&self, key: &str) {
        if let Some(code) = get_mac_keycode(key) {
            unsafe {
                let event = CGEventCreateKeyboardEvent(ptr::null_mut(), code, false);
                CGEventPost(K_CG_HID_EVENT_TAP, event);
                CFRelease(event);
            }
        }
    }
}

// --- KeyboardListener ---

pub struct KeyboardListenerCallbacks {
    pub on_press: Box<dyn Fn(String) + Send>,
    pub on_release: Box<dyn Fn(String) + Send>,
    pub suppress: bool,
}

pub struct KeyboardListener {
    runloop: Arc<Mutex<Option<SendRawPtr>>>,
    thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    callbacks_ptr: Arc<Mutex<Option<SendRawPtr>>>,
}

impl KeyboardListener {
    pub fn new<P, R>(on_press: P, on_release: R, suppress: bool) -> Self
    where
        P: Fn(String) + Send + 'static,
        R: Fn(String) + Send + 'static,
    {
        let callbacks = Box::into_raw(Box::new(KeyboardListenerCallbacks {
            on_press: Box::new(on_press),
            on_release: Box::new(on_release),
            suppress,
        }));

        Self {
            runloop: Arc::new(Mutex::new(None)),
            thread: Arc::new(Mutex::new(None)),
            callbacks_ptr: Arc::new(Mutex::new(Some(SendRawPtr(callbacks as *mut c_void)))),
        }
    }

    pub fn start(&self) {
        let runloop_clone = self.runloop.clone();
        let callbacks_ptr_clone = self.callbacks_ptr.clone();

        let handle = thread::spawn(move || unsafe {
            let rl = CFRunLoopGetCurrent();
            {
                let mut lock = runloop_clone.lock().unwrap();
                *lock = Some(SendRawPtr(rl));
            }

            let callbacks_raw = {
                let lock = callbacks_ptr_clone.lock().unwrap();
                lock.unwrap().0 as *mut KeyboardListenerCallbacks
            };

            let mask = (1 << K_CG_EVENT_KEY_DOWN)
                | (1 << K_CG_EVENT_KEY_UP)
                | (1 << K_CG_EVENT_FLAGS_CHANGED);

            let tap = CGEventTapCreate(
                K_CG_SESSION_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_DEFAULT,
                mask,
                keyboard_tap_callback,
                callbacks_raw as *mut c_void,
            );

            if tap.is_null() {
                log::error!(
                    "Failed to create keyboard event tap! Accessibility permissions are required."
                );
                return;
            }

            let source = CFMachPortCreateRunLoopSource(ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
            CGEventTapEnable(tap, true);

            CFRunLoopRun();

            CGEventTapEnable(tap, false);
            CFRelease(source);
            CFRelease(tap);
        });

        let mut thread_lock = self.thread.lock().unwrap();
        *thread_lock = Some(handle);
    }

    pub fn stop(&self) {
        {
            let rl_lock = self.runloop.lock().unwrap();
            if let Some(rl) = *rl_lock {
                unsafe {
                    CFRunLoopStop(rl.0);
                }
            }
        }

        let mut thread_lock = self.thread.lock().unwrap();
        if let Some(handle) = thread_lock.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for KeyboardListener {
    fn drop(&mut self) {
        self.stop();
        let mut ptr_lock = self.callbacks_ptr.lock().unwrap();
        if let Some(ptr) = ptr_lock.take() {
            unsafe {
                let _ = Box::from_raw(ptr.0 as *mut KeyboardListenerCallbacks);
            }
        }
    }
}

pub extern "C" fn keyboard_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    refcon: *mut c_void,
) -> CGEventRef {
    let callbacks = unsafe { &*(refcon as *mut KeyboardListenerCallbacks) };

    unsafe {
        let keycode = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE) as u16;
        let mut key_str = MACOS_KEY_MAP.get(&keycode).map(|s| s.to_string());

        if key_str.is_none() {
            if let Some(ch) = keycode_to_char_mac(keycode, 0) {
                key_str = Some(format!("'{}'", ch));
            } else {
                key_str = Some(format!("'{}'", (keycode as u8) as char));
            }
        }

        let key_name = key_str.unwrap_or_else(|| "Key.unknown".to_string());

        if event_type == K_CG_EVENT_FLAGS_CHANGED {
            let flags = CGEventGetFlags(event);
            let pressed = match keycode {
                55 | 54 => (flags & 0x0010_0000) != 0,
                56 | 60 => (flags & 0x0002_0000) != 0,
                59 | 62 => (flags & 0x0004_0000) != 0,
                58 | 61 => (flags & 0x0008_0000) != 0,
                57 => (flags & 0x0001_0000) != 0,
                _ => false,
            };

            if pressed {
                (callbacks.on_press)(key_name);
            } else {
                (callbacks.on_release)(key_name);
            }
            if callbacks.suppress {
                return ptr::null_mut();
            }
        } else if event_type == K_CG_EVENT_KEY_DOWN {
            (callbacks.on_press)(key_name);
            if callbacks.suppress {
                return ptr::null_mut();
            }
        } else if event_type == K_CG_EVENT_KEY_UP {
            (callbacks.on_release)(key_name);
            if callbacks.suppress {
                return ptr::null_mut();
            }
        }
    }

    event
}

// --- Clipboard ---

pub struct Clipboard;

impl Clipboard {
    pub fn data() -> String {
        let script_path = get_resource_path("getfiles.applescript");
        if let Ok(output) = Command::new("osascript").arg(&script_path).output() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !stdout.is_empty() {
                return stdout;
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
        let script_path = get_resource_path("file2clip.applescript");
        let mut cmd = Command::new("osascript");
        cmd.arg(&script_path);
        for f in files {
            cmd.arg(f);
        }
        let _ = cmd.output();
    }
}

static IGNORED_CHANGE_COUNT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);
static IN_SET_PROMISE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_promise_impl(id: &str, format: &str, size: usize) {
    log::info!("macOS Clipboard: Registering promise for id: {}, format: {}", id, format);
    
    IN_SET_PROMISE.store(true, Ordering::Relaxed);
    
    {
        let mut active = super::ACTIVE_PROMISE.lock().unwrap();
        *active = Some(super::PromiseContext {
            uuid: id.to_string(),
            format: format.to_string(),
            size,
            tx: None,
        });
    }
    
    let owner = GLOBAL_OWNER.0 as cocoa::base::id;
    
    unsafe {
        let pb: cocoa::base::id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
        let _: objc::runtime::BOOL = msg_send![pb, clearContents];
        
        let array: cocoa::base::id = msg_send![objc::class!(NSMutableArray), array];
        if format == "text" {
            let ns_type = to_nsstring("public.utf8-plain-text");
            let _: () = msg_send![array, addObject:ns_type];
            let _: () = msg_send![ns_type, release];
        } else if format == "files" {
            let ns_type = to_nsstring("NSFilenamesPboardType");
            let _: () = msg_send![array, addObject:ns_type];
            let _: () = msg_send![ns_type, release];
        }
        
        let _: libc::intptr_t = msg_send![pb, declareTypes:array owner:owner];
    }

    let new_count = get_pasteboard_change_count();
    IGNORED_CHANGE_COUNT.store(new_count, Ordering::Relaxed);
    IN_SET_PROMISE.store(false, Ordering::Relaxed);
}

use std::sync::mpsc::{Sender, channel};
use std::sync::atomic::{Ordering};
use std::time::Duration;
use objc::runtime::{Object, Sel};
use objc::declare::ClassDecl;

fn get_pasteboard_change_count() -> i64 {
    unsafe {
        let pb: cocoa::base::id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
        let count: libc::intptr_t = msg_send![pb, changeCount];
        count as i64
    }
}

pub struct ClipboardListener {
    running: Arc<Mutex<bool>>,
    thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
    on_change: Arc<dyn Fn() + Send + Sync + 'static>,
}

impl ClipboardListener {
    pub fn new<F>(on_change: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self {
            running: Arc::new(Mutex::new(false)),
            thread: Arc::new(Mutex::new(None)),
            on_change: Arc::new(on_change),
        }
    }

    pub fn start(&self) {
        let running = self.running.clone();
        *running.lock().unwrap() = true;
        let on_change = self.on_change.clone();

        let handle = thread::spawn(move || {
            let mut last_change_count = get_pasteboard_change_count();
            while *running.lock().unwrap() {
                thread::sleep(Duration::from_millis(250));
                
                if IN_SET_PROMISE.load(Ordering::Relaxed) || crate::hardware::IN_SET_CLIPBOARD.load(Ordering::Relaxed) {
                    continue;
                }
                
                let current_change_count = get_pasteboard_change_count();
                if current_change_count != last_change_count {
                    last_change_count = current_change_count;
                    
                    let ignored = IGNORED_CHANGE_COUNT.load(Ordering::Relaxed);
                    if current_change_count == ignored {
                        log::info!("ClipboardListener: Ignoring change count {} matching registered promise", current_change_count);
                        continue;
                    }
                    
                    on_change();
                }
            }
        });

        let mut thread_lock = self.thread.lock().unwrap();
        *thread_lock = Some(handle);
    }

    pub fn stop(&self) {
        {
            let mut running = self.running.lock().unwrap();
            *running = false;
        }
        let mut thread_lock = self.thread.lock().unwrap();
        if let Some(handle) = thread_lock.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ClipboardListener {
    fn drop(&mut self) {
        self.stop();
    }
}

static REGISTER_OWNER_CLASS: Lazy<()> = Lazy::new(|| {
    unsafe {
        let superclass = objc::class!(NSObject);
        let mut decl = ClassDecl::new("FlowPasteboardOwner", superclass).unwrap();
        decl.add_method(
            objc::sel!(pasteboard:provideDataForType:),
            provide_data as extern "C" fn(&Object, Sel, cocoa::base::id, cocoa::base::id),
        );
        decl.register();
    }
});

static GLOBAL_OWNER: Lazy<SendRawPtr> = Lazy::new(|| unsafe {
    let _ = *REGISTER_OWNER_CLASS;
    let owner: cocoa::base::id = msg_send![objc::class!(FlowPasteboardOwner), new];
    SendRawPtr(owner as *mut c_void)
});

extern "C" fn provide_data(_this: &Object, _cmd: Sel, pasteboard: cocoa::base::id, pb_type: cocoa::base::id) {
    let utf8_str: *const libc::c_char = unsafe { msg_send![pb_type, UTF8String] };
    let format_str = if !utf8_str.is_null() {
        unsafe { std::ffi::CStr::from_ptr(utf8_str).to_string_lossy().into_owned() }
    } else {
        return;
    };
    log::info!("macOS Clipboard FFI: Pasteboard requested format {}", format_str);

    let (uuid, rx) = {
        let mut active_lock = super::ACTIVE_PROMISE.lock().unwrap();
        if let Some(ref mut active) = *active_lock {
            let (tx, rx) = channel();
            active.tx = Some(tx);
            (active.uuid.clone(), rx)
        } else {
            return;
        }
    };

    {
        let callback_lock = super::PROMISE_REQUEST_CALLBACK.lock().unwrap();
        if let Some(ref callback) = *callback_lock {
            callback(uuid.clone());
        } else {
            log::error!("macOS Clipboard FFI: PROMISE_REQUEST_CALLBACK not initialized!");
            return;
        }
    }

    let timeout = Duration::from_secs(15);
    match rx.recv_timeout(timeout) {
        Ok(payload) => {
            let pool: cocoa::base::id = unsafe {
                let pool_cls = objc::class!(NSAutoreleasePool);
                msg_send![pool_cls, new]
            };

            let final_data_to_hash = match payload {
                super::FulfillmentPayload::Text(text) => {
                    unsafe {
                        let ns_str = to_nsstring(&text);
                        let ns_type = to_nsstring("public.utf8-plain-text");
                        let _: () = msg_send![pasteboard, setString:ns_str forType:ns_type];
                        let _: () = msg_send![ns_str, release];
                        let _: () = msg_send![ns_type, release];
                    }
                    Some(text)
                }
                super::FulfillmentPayload::Files(local_paths) => {
                    let joined = local_paths.join("\n");
                    unsafe {
                        let array: cocoa::base::id = msg_send![objc::class!(NSMutableArray), array];
                        for path in local_paths {
                            let ns_str = to_nsstring(&path);
                            let _: () = msg_send![array, addObject:ns_str];
                            let _: () = msg_send![ns_str, release];
                        }
                        let ns_type = to_nsstring("NSFilenamesPboardType");
                        let _: () = msg_send![pasteboard, setPropertyList:array forType:ns_type];
                        let _: () = msg_send![ns_type, release];
                    }
                    Some(joined)
                }
            };

            unsafe {
                let _: () = msg_send![pool, release];
            }

            if let Some(data_str) = final_data_to_hash {
                use std::hash::{Hash, Hasher};
                let mut hasher = rustc_hash::FxHasher::default();
                data_str.hash(&mut hasher);
                let h = hasher.finish();
                crate::hardware::push_ignore_hash(h);
                log::info!("macOS Clipboard FFI: Ignored promise hash {} to prevent loopback", h);
            }
        }
        Err(e) => {
            log::warn!("macOS Clipboard FFI: Timeout or disconnect waiting for clipboard fulfillment: {:?}", e);
            crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn to_nsstring(s: &str) -> cocoa::base::id {
    unsafe {
        let class = objc::class!(NSString);
        let bytes = s.as_bytes();
        let ns_str: cocoa::base::id = msg_send![class, alloc];
        let ns_str: cocoa::base::id = msg_send![ns_str, initWithBytes:bytes.as_ptr() length:bytes.len() encoding:4]; // 4 = NSUTF8StringEncoding
        ns_str
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_change_count_increments() {
        let original_clipboard = Clipboard::data();
        
        let c1 = get_pasteboard_change_count();
        Clipboard::set_text("test-change-count-text-1");
        thread::sleep(Duration::from_millis(150));
        let c2 = get_pasteboard_change_count();
        
        Clipboard::set_text(&original_clipboard);
        assert!(c2 > c1, "changeCount did not increment after set_text. c1: {}, c2: {}", c1, c2);
    }
}
