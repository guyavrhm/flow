use std::ffi::c_void;
use std::os::raw::c_ulong;
use std::sync::atomic::Ordering;
use std::sync::RwLock;
use std::time::Duration;
use once_cell::sync::Lazy;
use objc::{msg_send, sel, sel_impl};
use objc::runtime::{Object, Sel};
use objc::declare::ClassDecl;
use std::sync::mpsc::channel;

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

pub type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    refcon: *mut c_void,
) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    pub fn CGMainDisplayID() -> u32;
    pub fn CGDisplayBounds(display: u32) -> CGRect;
    pub fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    pub fn CGDisplayPixelsWide(display: u32) -> usize;
    pub fn CGDisplayPixelsHigh(display: u32) -> usize;
    pub fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    pub fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    pub fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    pub fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        mouse_cursor_position: CGPoint,
        mouse_button: u32,
    ) -> CGEventRef;
    pub fn CGEventCreateScrollWheelEvent(
        source: CGEventSourceRef,
        units: u32,
        mouse_button: u32,
        wheel1: i32,
        wheel2: i32,
    ) -> CGEventRef;
    pub fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        keycode: u16,
        key_down: bool,
    ) -> CGEventRef;
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventGetFlags(event: CGEventRef) -> u64;
    pub fn CGDisplayHideCursor(display: u32) -> i32;
    pub fn CGDisplayShowCursor(display: u32) -> i32;
    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        refcon: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: CFMachPortRef,
        index: isize,
    ) -> CFRunLoopSourceRef;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: *const c_void);
    pub fn CFRunLoopRun();
    pub fn CFRunLoopStop(rl: CFRunLoopRef);
    pub fn CFRelease(obj: *mut c_void);
    pub fn CFDataGetBytePtr(the_data: *mut c_void) -> *const u8;
    pub fn CFDataGetLength(the_data: *mut c_void) -> isize;
    pub static kCFRunLoopDefaultMode: *const c_void;
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub fn TISCopyCurrentKeyboardInputSource() -> *mut c_void;
    pub fn TISGetInputSourceProperty(source: *mut c_void, property_key: *const c_void) -> *mut c_void;
    pub fn UCKeyTranslate(
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
    pub static kTISPropertyUnicodeKeyLayoutData: *const c_void;
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

pub fn keycode_to_char_mac(keycode: u16, modifier_state: u32) -> Option<String> {
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

pub static IGNORED_CHANGE_COUNT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);
pub static IN_SET_PROMISE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn get_pasteboard_change_count() -> i64 {
    unsafe {
        let pb: cocoa::base::id = msg_send![objc::class!(NSPasteboard), generalPasteboard];
        let count: libc::intptr_t = msg_send![pb, changeCount];
        count as i64
    }
}

pub fn to_nsstring(s: &str) -> cocoa::base::id {
    unsafe {
        let class = objc::class!(NSString);
        let bytes = s.as_bytes();
        let ns_str: cocoa::base::id = msg_send![class, alloc];
        let ns_str: cocoa::base::id = msg_send![ns_str, initWithBytes:bytes.as_ptr() length:bytes.len() encoding:4]; // 4 = NSUTF8StringEncoding
        ns_str
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

pub static GLOBAL_OWNER: Lazy<SendRawPtr> = Lazy::new(|| unsafe {
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
        let mut active_lock = crate::hardware::ACTIVE_PROMISE.lock().unwrap();
        if let Some(ref mut active) = *active_lock {
            let (tx, rx) = channel();
            active.tx = Some(tx);
            (active.uuid.clone(), rx)
        } else {
            return;
        }
    };

    {
        let callback_lock = crate::hardware::PROMISE_REQUEST_CALLBACK.lock().unwrap();
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
                crate::hardware::FulfillmentPayload::Text(text) => {
                    unsafe {
                        let ns_str = to_nsstring(&text);
                        let ns_type = to_nsstring("public.utf8-plain-text");
                        let _: () = msg_send![pasteboard, setString:ns_str forType:ns_type];
                        let _: () = msg_send![ns_str, release];
                        let _: () = msg_send![ns_type, release];
                    }
                    Some(text)
                }
                crate::hardware::FulfillmentPayload::Files(local_paths) => {
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

pub fn set_promise_impl(id: &str, format: &str, size: usize) {
    log::info!("macOS Clipboard: Registering promise for id: {}, format: {}", id, format);
    
    IN_SET_PROMISE.store(true, Ordering::Relaxed);
    
    {
        let mut active = crate::hardware::ACTIVE_PROMISE.lock().unwrap();
        *active = Some(crate::hardware::PromiseContext {
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

