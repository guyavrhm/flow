// Linux Hardware Abstraction Layer implementation for flow
// Supports evdev & uinput for X11 & Wayland mouse/keyboard capture and injection
// Supports GTK3 event-driven selection clipboard with streaming promises

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, EventType, InputEvent, Key, RelativeAxisType,
    UinputAbsSetup,
};
use gtk::gdk;
use gtk::glib;
use gtk::glib::object::ObjectExt;
use gtk::prelude::*;
use once_cell::sync::Lazy;

use crate::hardware::{
    check_and_consume_ignore_hash, push_ignore_hash, FulfillmentPayload, PromiseContext,
    ACTIVE_PROMISE, IN_SET_CLIPBOARD, PROMISE_REQUEST_CALLBACK,
};
use crate::network::protocol::MonitorInfo;

// ---------------------------------------------------------------------------
// Global Virtual Input Devices (uinput)
// ---------------------------------------------------------------------------

static GLOBAL_VIRTUAL_MOUSE: Lazy<Mutex<Option<VirtualDevice>>> =
    Lazy::new(|| Mutex::new(init_virtual_mouse()));
static GLOBAL_VIRTUAL_KEYBOARD: Lazy<Mutex<Option<VirtualDevice>>> =
    Lazy::new(|| Mutex::new(init_virtual_keyboard()));

fn init_virtual_mouse() -> Option<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    keys.insert(Key::BTN_LEFT);
    keys.insert(Key::BTN_RIGHT);
    keys.insert(Key::BTN_MIDDLE);
    keys.insert(Key::BTN_SIDE);
    keys.insert(Key::BTN_EXTRA);

    let mut rel_axes = AttributeSet::<RelativeAxisType>::new();
    rel_axes.insert(RelativeAxisType::REL_X);
    rel_axes.insert(RelativeAxisType::REL_Y);
    rel_axes.insert(RelativeAxisType::REL_WHEEL);
    rel_axes.insert(RelativeAxisType::REL_HWHEEL);

    let abs_setup_x = UinputAbsSetup::new(AbsoluteAxisType::ABS_X, AbsInfo::new(0, 0, 32767, 0, 0, 0));
    let abs_setup_y = UinputAbsSetup::new(AbsoluteAxisType::ABS_Y, AbsInfo::new(0, 0, 32767, 0, 0, 0));

    match VirtualDeviceBuilder::new() {
        Ok(builder) => match builder
            .name("flow Virtual Mouse")
            .with_keys(&keys)
            .and_then(|b| b.with_relative_axes(&rel_axes))
            .and_then(|b| b.with_absolute_axis(&abs_setup_x))
            .and_then(|b| b.with_absolute_axis(&abs_setup_y))
            .and_then(|b| b.build())
        {
            Ok(dev) => {
                log::info!("Successfully created Linux /dev/uinput virtual mouse device");
                Some(dev)
            }
            Err(e) => {
                log::warn!("Failed to build /dev/uinput virtual mouse: {:?}. To grant permissions run: sudo usermod -aG input $USER && echo 'KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"input\", OPTIONS+=\"static_node=uinput\"' | sudo tee /etc/udev/rules.d/99-input.rules", e);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to initialize VirtualDeviceBuilder for mouse: {:?}.", e);
            None
        }
    }
}

fn init_virtual_keyboard() -> Option<VirtualDevice> {
    let mut keys = AttributeSet::<Key>::new();
    for code in 1..=255 {
        keys.insert(Key::new(code));
    }

    match VirtualDeviceBuilder::new() {
        Ok(builder) => match builder
            .name("flow Virtual Keyboard")
            .with_keys(&keys)
            .and_then(|b| b.build())
        {
            Ok(dev) => {
                log::info!("Successfully created Linux /dev/uinput virtual keyboard device");
                Some(dev)
            }
            Err(e) => {
                log::warn!("Failed to build /dev/uinput virtual keyboard: {:?}. To grant permissions run: sudo usermod -aG input $USER", e);
                None
            }
        },
        Err(e) => {
            log::warn!("Failed to initialize VirtualDeviceBuilder for keyboard: {:?}.", e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// MouseController Implementation (Simulation)
// ---------------------------------------------------------------------------

pub struct MouseController;

impl MouseController {
    pub fn new() -> Self {
        drop(GLOBAL_VIRTUAL_MOUSE.lock().unwrap());
        Self
    }

    pub fn position(&self) -> (i32, i32) {
        if !gtk::is_initialized() {
            return (0, 0);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        glib::MainContext::default().invoke(move || {
            let pos = if let Some(display) = gdk::Display::default() {
                if let Some(seat) = display.default_seat() {
                    if let Some(pointer) = seat.pointer() {
                        let (_, x, y) = pointer.position();
                        (x, y)
                    } else {
                        (0, 0)
                    }
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            let _ = tx.send(pos);
        });
        rx.recv_timeout(Duration::from_millis(100)).unwrap_or((0, 0))
    }

    pub fn set_position(&self, pos: (i32, i32)) {
        let (screen_w, screen_h) = get_screeninfo();
        let abs_x = if screen_w > 0 {
            ((pos.0 as f64 / screen_w as f64) * 32767.0).clamp(0.0, 32767.0) as i32
        } else {
            0
        };
        let abs_y = if screen_h > 0 {
            ((pos.1 as f64 / screen_h as f64) * 32767.0).clamp(0.0, 32767.0) as i32
        } else {
            0
        };

        if let Ok(mut guard) = GLOBAL_VIRTUAL_MOUSE.lock() {
            if let Some(ref mut vdev) = *guard {
                let ev_x = InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_X.0, abs_x);
                let ev_y = InputEvent::new(EventType::ABSOLUTE, AbsoluteAxisType::ABS_Y.0, abs_y);
                let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                let _ = vdev.emit(&[ev_x, ev_y, syn]);
            }
        }
    }

    pub fn press(&self, button: &str) {
        if let Some(key) = button_name_to_evdev(button) {
            if let Ok(mut guard) = GLOBAL_VIRTUAL_MOUSE.lock() {
                if let Some(ref mut vdev) = *guard {
                    let ev = InputEvent::new(EventType::KEY, key.code(), 1);
                    let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                    let _ = vdev.emit(&[ev, syn]);
                }
            }
        }
    }

    pub fn release(&self, button: &str) {
        if let Some(key) = button_name_to_evdev(button) {
            if let Ok(mut guard) = GLOBAL_VIRTUAL_MOUSE.lock() {
                if let Some(ref mut vdev) = *guard {
                    let ev = InputEvent::new(EventType::KEY, key.code(), 0);
                    let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                    let _ = vdev.emit(&[ev, syn]);
                }
            }
        }
    }

    pub fn scroll(&self, dx: i32, dy: i32) {
        if let Ok(mut guard) = GLOBAL_VIRTUAL_MOUSE.lock() {
            if let Some(ref mut vdev) = *guard {
                let mut events = Vec::new();
                if dx != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_HWHEEL.0,
                        dx,
                    ));
                }
                if dy != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE,
                        RelativeAxisType::REL_WHEEL.0,
                        dy,
                    ));
                }
                if !events.is_empty() {
                    events.push(InputEvent::new(EventType::SYNCHRONIZATION, 0, 0));
                    let _ = vdev.emit(&events);
                }
            }
        }
    }
}

impl crate::hardware::MouseSimulator for MouseController {
    fn position(&self) -> (i32, i32) {
        Self::position(self)
    }
    fn set_position(&self, pos: (i32, i32)) {
        Self::set_position(self, pos);
    }
    fn press(&self, button: &str) {
        Self::press(self, button);
    }
    fn release(&self, button: &str) {
        Self::release(self, button);
    }
    fn scroll(&self, dx: i32, dy: i32) {
        Self::scroll(self, dx, dy);
    }
}

fn button_name_to_evdev(name: &str) -> Option<Key> {
    match name.to_lowercase().as_str() {
        "left" => Some(Key::BTN_LEFT),
        "right" => Some(Key::BTN_RIGHT),
        "middle" => Some(Key::BTN_MIDDLE),
        "side" | "back" => Some(Key::BTN_SIDE),
        "extra" | "forward" => Some(Key::BTN_EXTRA),
        _ => None,
    }
}

fn evdev_code_to_button(code: u16) -> Option<String> {
    match Key::new(code) {
        Key::BTN_LEFT => Some("Left".to_string()),
        Key::BTN_RIGHT => Some("Right".to_string()),
        Key::BTN_MIDDLE => Some("Middle".to_string()),
        Key::BTN_SIDE => Some("Side".to_string()),
        Key::BTN_EXTRA => Some("Extra".to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// KeyboardController Implementation (Simulation)
// ---------------------------------------------------------------------------

pub struct KeyboardController;

impl KeyboardController {
    pub fn new() -> Self {
        drop(GLOBAL_VIRTUAL_KEYBOARD.lock().unwrap());
        Self
    }

    pub fn press(&self, key: &str) {
        if let Some(evdev_key) = key_name_to_evdev_key(key) {
            if let Ok(mut guard) = GLOBAL_VIRTUAL_KEYBOARD.lock() {
                if let Some(ref mut vdev) = *guard {
                    let ev = InputEvent::new(EventType::KEY, evdev_key.code(), 1);
                    let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                    let _ = vdev.emit(&[ev, syn]);
                }
            }
        }
    }

    pub fn release(&self, key: &str) {
        if let Some(evdev_key) = key_name_to_evdev_key(key) {
            if let Ok(mut guard) = GLOBAL_VIRTUAL_KEYBOARD.lock() {
                if let Some(ref mut vdev) = *guard {
                    let ev = InputEvent::new(EventType::KEY, evdev_key.code(), 0);
                    let syn = InputEvent::new(EventType::SYNCHRONIZATION, 0, 0);
                    let _ = vdev.emit(&[ev, syn]);
                }
            }
        }
    }

    pub fn release_all(&self) {
        for modifier in &[
            "Shift", "Shift_R", "Control", "Control_R", "Alt", "Alt_R", "Meta", "Meta_R",
        ] {
            self.release(modifier);
        }
    }
}

impl crate::hardware::KeyboardSimulator for KeyboardController {
    fn press(&self, key: &str) {
        Self::press(self, key);
    }
    fn release(&self, key: &str) {
        Self::release(self, key);
    }
    fn release_all(&self) {
        Self::release_all(self);
    }
}

fn key_name_to_evdev_key(name: &str) -> Option<Key> {
    match name {
        "a" | "A" => Some(Key::KEY_A),
        "b" | "B" => Some(Key::KEY_B),
        "c" | "C" => Some(Key::KEY_C),
        "d" | "D" => Some(Key::KEY_D),
        "e" | "E" => Some(Key::KEY_E),
        "f" | "F" => Some(Key::KEY_F),
        "g" | "G" => Some(Key::KEY_G),
        "h" | "H" => Some(Key::KEY_H),
        "i" | "I" => Some(Key::KEY_I),
        "j" | "J" => Some(Key::KEY_J),
        "k" | "K" => Some(Key::KEY_K),
        "l" | "L" => Some(Key::KEY_L),
        "m" | "M" => Some(Key::KEY_M),
        "n" | "N" => Some(Key::KEY_N),
        "o" | "O" => Some(Key::KEY_O),
        "p" | "P" => Some(Key::KEY_P),
        "q" | "Q" => Some(Key::KEY_Q),
        "r" | "R" => Some(Key::KEY_R),
        "s" | "S" => Some(Key::KEY_S),
        "t" | "T" => Some(Key::KEY_T),
        "u" | "U" => Some(Key::KEY_U),
        "v" | "V" => Some(Key::KEY_V),
        "w" | "W" => Some(Key::KEY_W),
        "x" | "X" => Some(Key::KEY_X),
        "y" | "Y" => Some(Key::KEY_Y),
        "z" | "Z" => Some(Key::KEY_Z),
        "0" => Some(Key::KEY_0),
        "1" => Some(Key::KEY_1),
        "2" => Some(Key::KEY_2),
        "3" => Some(Key::KEY_3),
        "4" => Some(Key::KEY_4),
        "5" => Some(Key::KEY_5),
        "6" => Some(Key::KEY_6),
        "7" => Some(Key::KEY_7),
        "8" => Some(Key::KEY_8),
        "9" => Some(Key::KEY_9),
        "Space" | " " => Some(Key::KEY_SPACE),
        "Enter" | "Return" => Some(Key::KEY_ENTER),
        "Tab" => Some(Key::KEY_TAB),
        "Escape" => Some(Key::KEY_ESC),
        "Backspace" => Some(Key::KEY_BACKSPACE),
        "Delete" => Some(Key::KEY_DELETE),
        "Shift" | "Shift_L" => Some(Key::KEY_LEFTSHIFT),
        "Shift_R" => Some(Key::KEY_RIGHTSHIFT),
        "Control" | "Control_L" => Some(Key::KEY_LEFTCTRL),
        "Control_R" => Some(Key::KEY_RIGHTCTRL),
        "Alt" | "Alt_L" => Some(Key::KEY_LEFTALT),
        "Alt_R" => Some(Key::KEY_RIGHTALT),
        "Meta" | "Meta_L" | "Super" | "Super_L" => Some(Key::KEY_LEFTMETA),
        "Meta_R" | "Super_R" => Some(Key::KEY_RIGHTMETA),
        "Up" => Some(Key::KEY_UP),
        "Down" => Some(Key::KEY_DOWN),
        "Left" => Some(Key::KEY_LEFT),
        "Right" => Some(Key::KEY_RIGHT),
        "Home" => Some(Key::KEY_HOME),
        "End" => Some(Key::KEY_END),
        "PageUp" => Some(Key::KEY_PAGEUP),
        "PageDown" => Some(Key::KEY_PAGEDOWN),
        "F1" => Some(Key::KEY_F1),
        "F2" => Some(Key::KEY_F2),
        "F3" => Some(Key::KEY_F3),
        "F4" => Some(Key::KEY_F4),
        "F5" => Some(Key::KEY_F5),
        "F6" => Some(Key::KEY_F6),
        "F7" => Some(Key::KEY_F7),
        "F8" => Some(Key::KEY_F8),
        "F9" => Some(Key::KEY_F9),
        "F10" => Some(Key::KEY_F10),
        "F11" => Some(Key::KEY_F11),
        "F12" => Some(Key::KEY_F12),
        "-" | "_" => Some(Key::KEY_MINUS),
        "=" | "+" => Some(Key::KEY_EQUAL),
        "[" | "{" => Some(Key::KEY_LEFTBRACE),
        "]" | "}" => Some(Key::KEY_RIGHTBRACE),
        ";" | ":" => Some(Key::KEY_SEMICOLON),
        "'" | "\"" => Some(Key::KEY_APOSTROPHE),
        "`" | "~" => Some(Key::KEY_GRAVE),
        "\\" | "|" => Some(Key::KEY_BACKSLASH),
        "," | "<" => Some(Key::KEY_COMMA),
        "." | ">" => Some(Key::KEY_DOT),
        "/" | "?" => Some(Key::KEY_SLASH),
        _ => None,
    }
}

fn evdev_key_to_name(key: Key) -> String {
    match key {
        Key::KEY_A => "a".to_string(),
        Key::KEY_B => "b".to_string(),
        Key::KEY_C => "c".to_string(),
        Key::KEY_D => "d".to_string(),
        Key::KEY_E => "e".to_string(),
        Key::KEY_F => "f".to_string(),
        Key::KEY_G => "g".to_string(),
        Key::KEY_H => "h".to_string(),
        Key::KEY_I => "i".to_string(),
        Key::KEY_J => "j".to_string(),
        Key::KEY_K => "k".to_string(),
        Key::KEY_L => "l".to_string(),
        Key::KEY_M => "m".to_string(),
        Key::KEY_N => "n".to_string(),
        Key::KEY_O => "o".to_string(),
        Key::KEY_P => "p".to_string(),
        Key::KEY_Q => "q".to_string(),
        Key::KEY_R => "r".to_string(),
        Key::KEY_S => "s".to_string(),
        Key::KEY_T => "t".to_string(),
        Key::KEY_U => "u".to_string(),
        Key::KEY_V => "v".to_string(),
        Key::KEY_W => "w".to_string(),
        Key::KEY_X => "x".to_string(),
        Key::KEY_Y => "y".to_string(),
        Key::KEY_Z => "z".to_string(),
        Key::KEY_0 => "0".to_string(),
        Key::KEY_1 => "1".to_string(),
        Key::KEY_2 => "2".to_string(),
        Key::KEY_3 => "3".to_string(),
        Key::KEY_4 => "4".to_string(),
        Key::KEY_5 => "5".to_string(),
        Key::KEY_6 => "6".to_string(),
        Key::KEY_7 => "7".to_string(),
        Key::KEY_8 => "8".to_string(),
        Key::KEY_9 => "9".to_string(),
        Key::KEY_SPACE => "Space".to_string(),
        Key::KEY_ENTER => "Enter".to_string(),
        Key::KEY_TAB => "Tab".to_string(),
        Key::KEY_ESC => "Escape".to_string(),
        Key::KEY_BACKSPACE => "Backspace".to_string(),
        Key::KEY_DELETE => "Delete".to_string(),
        Key::KEY_LEFTSHIFT => "Shift".to_string(),
        Key::KEY_RIGHTSHIFT => "Shift_R".to_string(),
        Key::KEY_LEFTCTRL => "Control".to_string(),
        Key::KEY_RIGHTCTRL => "Control_R".to_string(),
        Key::KEY_LEFTALT => "Alt".to_string(),
        Key::KEY_RIGHTALT => "Alt_R".to_string(),
        Key::KEY_LEFTMETA => "Meta".to_string(),
        Key::KEY_RIGHTMETA => "Meta_R".to_string(),
        Key::KEY_UP => "Up".to_string(),
        Key::KEY_DOWN => "Down".to_string(),
        Key::KEY_LEFT => "Left".to_string(),
        Key::KEY_RIGHT => "Right".to_string(),
        Key::KEY_HOME => "Home".to_string(),
        Key::KEY_END => "End".to_string(),
        Key::KEY_PAGEUP => "PageUp".to_string(),
        Key::KEY_PAGEDOWN => "PageDown".to_string(),
        Key::KEY_F1 => "F1".to_string(),
        Key::KEY_F2 => "F2".to_string(),
        Key::KEY_F3 => "F3".to_string(),
        Key::KEY_F4 => "F4".to_string(),
        Key::KEY_F5 => "F5".to_string(),
        Key::KEY_F6 => "F6".to_string(),
        Key::KEY_F7 => "F7".to_string(),
        Key::KEY_F8 => "F8".to_string(),
        Key::KEY_F9 => "F9".to_string(),
        Key::KEY_F10 => "F10".to_string(),
        Key::KEY_F11 => "F11".to_string(),
        Key::KEY_F12 => "F12".to_string(),
        Key::KEY_MINUS => "-".to_string(),
        Key::KEY_EQUAL => "=".to_string(),
        Key::KEY_LEFTBRACE => "[".to_string(),
        Key::KEY_RIGHTBRACE => "]".to_string(),
        Key::KEY_SEMICOLON => ";".to_string(),
        Key::KEY_APOSTROPHE => "'".to_string(),
        Key::KEY_GRAVE => "`".to_string(),
        Key::KEY_BACKSLASH => "\\".to_string(),
        Key::KEY_COMMA => ",".to_string(),
        Key::KEY_DOT => ".".to_string(),
        Key::KEY_SLASH => "/".to_string(),
        _ => format!("{:?}", key),
    }
}

// ---------------------------------------------------------------------------
// Evdev Mouse & Keyboard Listeners (Interception & Swallowing)
// ---------------------------------------------------------------------------

pub struct MouseListener {
    running: Arc<Mutex<bool>>,
    thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl MouseListener {
    pub fn new<M, C, S>(on_move: M, on_click: C, on_scroll: S, suppress: bool) -> Self
    where
        M: Fn(i32, i32) + Send + 'static,
        C: Fn(i32, i32, String, bool) + Send + 'static,
        S: Fn(i32, i32, i32, i32) + Send + 'static,
    {
        let running = Arc::new(Mutex::new(false));
        let running_clone = running.clone();

        let thread_handle = thread::spawn(move || {
            let mut devices = Vec::new();
            for (_path, dev) in evdev::enumerate() {
                let has_rel = dev.supported_relative_axes().map_or(false, |axes| {
                    axes.contains(RelativeAxisType::REL_X)
                });
                let has_btn = dev.supported_keys().map_or(false, |keys| {
                    keys.contains(Key::BTN_LEFT)
                });
                if has_rel || has_btn {
                    devices.push(dev);
                }
            }

            let mut is_currently_grabbed = false;
            *running_clone.lock().unwrap() = true;
            log::info!("Started evdev MouseListener monitoring {} devices", devices.len());

            let mut mouse_x = 0i32;
            let mut mouse_y = 0i32;

            while *running_clone.lock().unwrap() {
                let should_redirect = crate::state::IS_REDIRECTING.load(std::sync::atomic::Ordering::Relaxed);
                if suppress {
                    if should_redirect && !is_currently_grabbed {
                        for dev in &mut devices {
                            let _ = dev.grab();
                        }
                        is_currently_grabbed = true;
                    } else if !should_redirect && is_currently_grabbed {
                        for dev in &mut devices {
                            let _ = dev.ungrab();
                        }
                        is_currently_grabbed = false;
                    }
                }

                if !should_redirect {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                let mut activity = false;
                for dev in &mut devices {
                    if let Ok(events) = dev.fetch_events() {
                        for ev in events {
                            activity = true;
                            match ev.event_type() {
                                EventType::RELATIVE => {
                                    let rel_axis = RelativeAxisType(ev.code());
                                    let val = ev.value();
                                    if rel_axis == RelativeAxisType::REL_X || rel_axis == RelativeAxisType::REL_Y {
                                        let dx = if rel_axis == RelativeAxisType::REL_X { val } else { 0 };
                                        let dy = if rel_axis == RelativeAxisType::REL_Y { val } else { 0 };
                                        mouse_x += dx;
                                        mouse_y += dy;
                                        on_move(dx, dy);
                                    } else if rel_axis == RelativeAxisType::REL_WHEEL || rel_axis == RelativeAxisType::REL_HWHEEL {
                                        let dx = if rel_axis == RelativeAxisType::REL_HWHEEL { val } else { 0 };
                                        let dy = if rel_axis == RelativeAxisType::REL_WHEEL { val } else { 0 };
                                        on_scroll(mouse_x, mouse_y, dx, dy);
                                    }
                                }
                                EventType::KEY => {
                                    if let Some(btn_str) = evdev_code_to_button(ev.code()) {
                                        let pressed = ev.value() != 0;
                                        on_click(mouse_x, mouse_y, btn_str, pressed);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                if !activity {
                    thread::sleep(Duration::from_millis(5));
                }
            }

            if is_currently_grabbed {
                for dev in &mut devices {
                    let _ = dev.ungrab();
                }
            }
            log::info!("Stopped evdev MouseListener");
        });

        Self {
            running,
            thread: Arc::new(Mutex::new(Some(thread_handle))),
        }
    }

    pub fn start(&self) {
        // Started upon thread spawn in new()
    }

    pub fn stop(&self) {
        *self.running.lock().unwrap() = false;
    }

    pub fn join(&self) {
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

impl crate::hardware::InputHookListener for MouseListener {
    fn start(&self) {
        Self::start(self);
    }
    fn stop(&self) {
        Self::stop(self);
    }
}

pub struct KeyboardListener {
    running: Arc<Mutex<bool>>,
    thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl KeyboardListener {
    pub fn new<P, R>(on_press: P, on_release: R, suppress: bool) -> Self
    where
        P: Fn(String) + Send + 'static,
        R: Fn(String) + Send + 'static,
    {
        let running = Arc::new(Mutex::new(false));
        let running_clone = running.clone();

        let thread_handle = thread::spawn(move || {
            let mut devices = Vec::new();
            for (_path, dev) in evdev::enumerate() {
                let has_key_a = dev.supported_keys().map_or(false, |keys| {
                    keys.contains(Key::KEY_A)
                });
                if has_key_a {
                    devices.push(dev);
                }
            }

            let mut is_currently_grabbed = false;
            *running_clone.lock().unwrap() = true;
            log::info!("Started evdev KeyboardListener monitoring {} devices", devices.len());

            let mut ctrl_down = false;
            let mut alt_down = false;

            while *running_clone.lock().unwrap() {
                let should_redirect = crate::state::IS_REDIRECTING.load(std::sync::atomic::Ordering::Relaxed);
                if suppress {
                    if should_redirect && !is_currently_grabbed {
                        for dev in &mut devices {
                            let _ = dev.grab();
                        }
                        is_currently_grabbed = true;
                    } else if !should_redirect && is_currently_grabbed {
                        for dev in &mut devices {
                            let _ = dev.ungrab();
                        }
                        is_currently_grabbed = false;
                    }
                }

                if !should_redirect {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }

                let mut activity = false;
                for dev in &mut devices {
                    if let Ok(events) = dev.fetch_events() {
                        for ev in events {
                            activity = true;
                            if ev.event_type() == EventType::KEY {
                                let key_code = ev.code();
                                let is_down = ev.value() != 0;
                                if key_code == Key::KEY_LEFTCTRL.code() || key_code == Key::KEY_RIGHTCTRL.code() {
                                    ctrl_down = is_down;
                                } else if key_code == Key::KEY_LEFTALT.code() || key_code == Key::KEY_RIGHTALT.code() {
                                    alt_down = is_down;
                                }

                                // Emergency escape: Ctrl + Alt + Escape
                                if key_code == Key::KEY_ESC.code() && is_down && ctrl_down && alt_down {
                                    log::warn!("[EMERGENCY] Ctrl+Alt+Escape detected on Linux! Triggering emergency release to host.");
                                    crate::state::STATE_MANAGER.emergency_release();
                                    continue;
                                }

                                let key_name = evdev_key_to_name(Key::new(key_code));
                                match ev.value() {
                                    1 | 2 => on_press(key_name),
                                    0 => on_release(key_name),
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if !activity {
                    thread::sleep(Duration::from_millis(5));
                }
            }

            if is_currently_grabbed {
                for dev in &mut devices {
                    let _ = dev.ungrab();
                }
            }
            log::info!("Stopped evdev KeyboardListener");
        });

        Self {
            running,
            thread: Arc::new(Mutex::new(Some(thread_handle))),
        }
    }

    pub fn start(&self) {
        // Started upon thread spawn in new()
    }

    pub fn stop(&self) {
        *self.running.lock().unwrap() = false;
    }

    pub fn join(&self) {
        if let Some(handle) = self.thread.lock().unwrap().take() {
            let _ = handle.join();
        }
    }
}

impl crate::hardware::InputHookListener for KeyboardListener {
    fn start(&self) {
        Self::start(self);
    }
    fn stop(&self) {
        Self::stop(self);
    }
}

// ---------------------------------------------------------------------------
// GTK Clipboard Management & Event-Driven Promises
// ---------------------------------------------------------------------------

pub struct ClipboardController;

impl ClipboardController {
    pub fn data() -> String {
        if !gtk::is_initialized() {
            return String::new();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        glib::MainContext::default().invoke(move || {
            let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            let text = clipboard.wait_for_text().map(|s| s.to_string()).unwrap_or_default();
            let _ = tx.send(text);
        });
        rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default()
    }

    pub fn set_text(text: &str) {
        if !gtk::is_initialized() {
            return;
        }
        let text_owned = text.to_string();
        let hash = compute_text_hash(&text_owned);
        push_ignore_hash(hash);

        IN_SET_CLIPBOARD.store(true, Ordering::SeqCst);
        glib::MainContext::default().invoke(move || {
            let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            clipboard.set_text(&text_owned);
            IN_SET_CLIPBOARD.store(false, Ordering::SeqCst);
        });
    }

    pub fn set_files(files: Vec<String>) {
        if !gtk::is_initialized() {
            return;
        }
        let uris: Vec<String> = files
            .into_iter()
            .map(|f| format!("file://{}", f))
            .collect();
        let hash = compute_text_hash(&uris.join("\n"));
        push_ignore_hash(hash);

        IN_SET_CLIPBOARD.store(true, Ordering::SeqCst);
        glib::MainContext::default().invoke(move || {
            let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
            clipboard.set_text(&uris.join("\n"));
            IN_SET_CLIPBOARD.store(false, Ordering::SeqCst);
        });
    }
}

impl crate::hardware::ClipboardManager for ClipboardController {
    fn data(&self) -> String {
        Self::data()
    }
    fn set_text(&self, text: &str) {
        Self::set_text(text);
    }
    fn set_files(&self, files: Vec<String>) {
        Self::set_files(files);
    }
    fn set_promise(&self, id: &str, format: &str, size: usize) {
        set_promise_impl(id, format, size);
    }
}

pub(crate) fn set_promise_impl(id: &str, format: &str, size: usize) {
    if !gtk::is_initialized() {
        return;
    }
    let uuid = id.to_string();
    let fmt = format.to_string();

    let (tx, rx) = std::sync::mpsc::channel();
    let rx_arc = Arc::new(Mutex::new(rx));
    {
        let mut active = ACTIVE_PROMISE.lock().unwrap();
        *active = Some(PromiseContext {
            uuid: uuid.clone(),
            format: fmt.clone(),
            size,
            tx: Some(tx),
        });
    }

    glib::MainContext::default().invoke(move || {
        let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
        let targets = vec![
            gtk::TargetEntry::new("text/plain", gtk::TargetFlags::OTHER_APP, 0),
            gtk::TargetEntry::new("UTF8_STRING", gtk::TargetFlags::OTHER_APP, 1),
        ];

        let uuid_for_cb = uuid.clone();
        let rx_for_cb = rx_arc.clone();
        clipboard.set_with_data(&targets, move |_clipboard, selection, _info| {
            log::info!("GTK Promise selection requested by target application. Requesting network payload...");
            if let Some(ref cb) = *PROMISE_REQUEST_CALLBACK.lock().unwrap() {
                cb(uuid_for_cb.clone());
            }

            if let Ok(rx_guard) = rx_for_cb.lock() {
                if let Ok(payload) = rx_guard.recv_timeout(Duration::from_secs(15)) {
                    match payload {
                        FulfillmentPayload::Text(text) => {
                            selection.set_text(&text);
                        }
                        FulfillmentPayload::Files(files) => {
                            let uris: Vec<String> = files.into_iter().map(|f| format!("file://{}", f)).collect();
                            selection.set_text(&uris.join("\n"));
                        }
                    }
                }
            }
        });
    });
}

fn compute_text_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

// ---------------------------------------------------------------------------
// Event-Driven ClipboardListener
// ---------------------------------------------------------------------------

pub struct ClipboardListener {
    running: Arc<Mutex<bool>>,
}

impl ClipboardListener {
    pub fn new<F>(on_change: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        let running = Arc::new(Mutex::new(false));
        if gtk::is_initialized() {
            let on_change_arc = Arc::new(on_change);
            glib::MainContext::default().invoke(move || {
                let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
                let on_change_cb = on_change_arc.clone();
                clipboard.connect("owner-change", false, move |_| {
                    if IN_SET_CLIPBOARD.load(Ordering::SeqCst) {
                        return None;
                    }
                    let text = ClipboardController::data();
                    if !text.is_empty() {
                        let hash = compute_text_hash(&text);
                        if check_and_consume_ignore_hash(hash) {
                            return None;
                        }
                    }
                    on_change_cb();
                    None
                });
            });
        }

        Self { running }
    }

    pub fn start(&self) {
        *self.running.lock().unwrap() = true;
    }

    pub fn stop(&self) {
        *self.running.lock().unwrap() = false;
    }
}

// ---------------------------------------------------------------------------
// Display & Monitor Metrics
// ---------------------------------------------------------------------------

pub fn get_screeninfo() -> (i32, i32) {
    if !gtk::is_initialized() {
        return (1920, 1080);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    glib::MainContext::default().invoke(move || {
        let res = if let Some(display) = gdk::Display::default() {
            if let Some(mon) = display.primary_monitor().or_else(|| display.monitor(0)) {
                let rect = mon.geometry();
                (rect.width(), rect.height())
            } else {
                (1920, 1080)
            }
        } else {
            (1920, 1080)
        };
        let _ = tx.send(res);
    });
    rx.recv_timeout(Duration::from_millis(200)).unwrap_or((1920, 1080))
}

pub fn get_monitors() -> Vec<MonitorInfo> {
    if !gtk::is_initialized() {
        return vec![MonitorInfo {
            name: "Main Display".to_string(),
            local_x: 0,
            local_y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        }];
    }
    let (tx, rx) = std::sync::mpsc::channel();
    glib::MainContext::default().invoke(move || {
        let mut monitors = Vec::new();
        if let Some(display) = gdk::Display::default() {
            let n_monitors = display.n_monitors();
            for i in 0..n_monitors {
                if let Some(mon) = display.monitor(i) {
                    let rect = mon.geometry();
                    let scale = mon.scale_factor();
                    let name = mon.model().map(|s| s.to_string()).unwrap_or_else(|| format!("Monitor {}", i));
                    monitors.push(MonitorInfo {
                        name,
                        local_x: rect.x(),
                        local_y: rect.y(),
                        width: rect.width(),
                        height: rect.height(),
                        scale_factor: scale as f64,
                    });
                }
            }
        }
        let _ = tx.send(monitors);
    });
    let monitors = rx.recv_timeout(Duration::from_millis(200)).unwrap_or_default();
    if monitors.is_empty() {
        vec![MonitorInfo {
            name: "Main Display".to_string(),
            local_x: 0,
            local_y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        }]
    } else {
        monitors
    }
}

pub fn init_keyboard_layout() {
    log::info!("Linux keyboard layout initialized");
}

pub fn show_cursor() {}
pub fn hide_cursor() {}
pub fn uses_physical_pixels() -> bool {
    true
}

