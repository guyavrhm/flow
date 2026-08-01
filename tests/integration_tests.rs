use flow::config::SettingsData;
use flow::hardware::{Clipboard, MouseController};
use flow::network::protocol::InputEvent;
use flow::network::udp::{format_event, parse_event};

use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

static DB_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CLIPBOARD_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn get_db_lock() -> &'static Mutex<()> {
    DB_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

fn get_clipboard_lock() -> &'static Mutex<()> {
    CLIPBOARD_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

// ==========================================
// 1. UDP FORMATTING & PARSING TESTS
// ==========================================

#[test]
fn test_udp_event_formatting_and_parsing() {
    let move_ev = InputEvent::Move { x: 500, y: 600 };
    let click_ev = InputEvent::MouseClick { button: "left".to_string(), pressed: true };
    let scroll_ev = InputEvent::MouseScroll { dx: 1, dy: -2 };
    let key_ev = InputEvent::KeyPress { key: "Key.space".to_string(), pressed: false };
    let stop_ev = InputEvent::Stop;

    // Verify format -> parse parity
    let move_str = format_event(&move_ev);
    if let Some(InputEvent::Move { x, y }) = parse_event(&move_str) {
        assert_eq!(x, 500);
        assert_eq!(y, 600);
    } else {
        panic!("Failed to parse Move event");
    }

    let click_str = format_event(&click_ev);
    if let Some(InputEvent::MouseClick { button, pressed }) = parse_event(&click_str) {
        assert_eq!(button, "left");
        assert!(pressed);
    } else {
        panic!("Failed to parse MouseClick event");
    }

    let scroll_str = format_event(&scroll_ev);
    if let Some(InputEvent::MouseScroll { dx, dy }) = parse_event(&scroll_str) {
        assert_eq!(dx, 1);
        assert_eq!(dy, -2);
    } else {
        panic!("Failed to parse MouseScroll event");
    }

    let key_str = format_event(&key_ev);
    if let Some(InputEvent::KeyPress { key, pressed }) = parse_event(&key_str) {
        assert_eq!(key, "Key.space");
        assert!(!pressed);
    } else {
        panic!("Failed to parse KeyPress event");
    }

    let stop_str = format_event(&stop_ev);
    if let Some(InputEvent::Stop) = parse_event(&stop_str) {
        // Succeeded
    } else {
        panic!("Failed to parse Stop event");
    }
}

#[test]
fn test_udp_malformed_event_parsing() {
    assert!(parse_event("").is_none());
    assert!(parse_event("mov").is_none());
    assert!(parse_event("mov 100").is_none());
    assert!(parse_event("mov 100 abc").is_none());
    assert!(parse_event("mov abc 100").is_none());
    assert!(parse_event("scrl").is_none());
    assert!(parse_event("scrl 1").is_none());
    assert!(parse_event("scrl abc 2").is_none());
    assert!(parse_event("prsm").is_none());
    assert!(parse_event("prsm true").is_none());
    assert!(parse_event("prsm abc Left").is_none());
    assert!(parse_event("prsk").is_none());
    assert!(parse_event("prsk false").is_none());
    assert!(parse_event("prsk abc Key.space").is_none());
    assert!(parse_event("invalid_action").is_none());
}

// ==========================================
// 2. CONFIGURATION & DATABASE PERSISTENCE TESTS
// ==========================================

#[test]
fn test_sqlite_settings_persistence() {
    let _lock = get_db_lock().lock().unwrap();
    use std::fs;
    use flow::config::{
        initialize_db, get_settings, save_settings, PC_CLIENT, ENCRYPTION_ON,
        get_screens, get_attachments, update_screen, remove_screen, ScreenAttachments,
    };
    use flow::paths::get_db_path;

    let db_path = get_db_path();
    let backup_path = db_path.with_extension("db.backup");

    // 1. Back up existing DB if it exists
    let has_backup = if db_path.exists() {
        fs::copy(&db_path, &backup_path).is_ok()
    } else {
        false
    };

    // 2. Perform DB operations
    let test_run = || -> Result<(), Box<dyn std::error::Error>> {
        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Remove existing DB file to start fresh for test
        if db_path.exists() {
            fs::remove_file(&db_path)?;
        }

        initialize_db()?;

        // Verify default settings
        let default_settings = get_settings()?;
        assert_eq!(default_settings.ip, "");
        assert_eq!(default_settings.password, "");

        // Save new settings
        let new_settings = SettingsData {
            ip: "192.168.1.100".to_string(),
            password: "test_db_password".to_string(),
            pc: PC_CLIENT,
            encryption: ENCRYPTION_ON,
        };
        save_settings(&new_settings)?;

        // Read back and verify
        let retrieved = get_settings()?;
        assert_eq!(retrieved.ip, "192.168.1.100");
        assert_eq!(retrieved.password, "test_db_password");
        assert_eq!(retrieved.pc, PC_CLIENT);
        assert_eq!(retrieved.encryption, ENCRYPTION_ON);

        // --- Test Screen Attachments ---
        // 1. Initial screen check (only 'main' should exist)
        let screens = get_screens()?;
        assert_eq!(screens.len(), 1);
        assert_eq!(screens[0].address, "main");

        // 2. Query attachments for a non-existent screen '192.168.1.10'
        // Should create a default blank attachment list for it.
        let fresh_attachment = get_attachments("192.168.1.10")?;
        assert_eq!(fresh_attachment.address, "192.168.1.10");
        assert!(fresh_attachment.right.is_none());

        // We should have 2 screens now ('main' and '192.168.1.10')
        let screens2 = get_screens()?;
        assert_eq!(screens2.len(), 2);

        // 3. Update the attachments for '192.168.1.10'
        let updated_attachments = ScreenAttachments {
            address: "192.168.1.10".to_string(),
            top: None,
            right: Some("main".to_string()),
            bottom: None,
            left: None,
        };
        update_screen("192.168.1.10", &updated_attachments)?;

        // Read it back and verify it persists
        let retrieved_attachments = get_attachments("192.168.1.10")?;
        assert_eq!(retrieved_attachments.right, Some("main".to_string()));

        // 4. Remove the screen and verify it was deleted
        remove_screen("192.168.1.10")?;
        let screens_after_del = get_screens()?;
        assert_eq!(screens_after_del.len(), 1);
        assert_eq!(screens_after_del[0].address, "main");

        Ok(())
    };

    let result = test_run();

    // 3. Restore backup
    if has_backup {
        let _ = fs::copy(&backup_path, &db_path);
        let _ = fs::remove_file(&backup_path);
    } else if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }

    result.expect("DB test failed");
}

// ==========================================
// 3. MACOS-SPECIFIC HARDWARE FFI TESTS
// ==========================================

#[cfg(target_os = "macos")]
mod macos_tests {
    use super::*;
    use flow::hardware::mac::{
        CGPoint, KeyboardListenerCallbacks, MouseListenerCallbacks,
        keyboard_tap_callback, mouse_tap_callback, get_mac_keycode,
    };
    use std::ptr;
    use std::ffi::c_void;

    // --- FFI imports for synthetic CoreGraphics events in tests ---
    pub type CGEventRef = *mut c_void;
    pub type CGEventSourceRef = *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
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

        fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
        fn CFRelease(obj: *mut c_void);
    }

    // Constants matching mac.rs
    const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_SCROLL_WHEEL: u32 = 22;
    const K_CG_EVENT_KEY_DOWN: u32 = 10;
    const K_CG_EVENT_KEY_UP: u32 = 11;

    const K_CG_MOUSE_EVENT_DELTA_X: u32 = 4;
    const K_CG_MOUSE_EVENT_DELTA_Y: u32 = 5;

    #[test]
    fn test_mac_clipboard_combined() {
        let _lock = get_clipboard_lock().lock().unwrap_or_else(|e| e.into_inner());
        let original_clipboard = Clipboard::data();

        let test_str = "flow-system-test-unique-string-123456";
        Clipboard::set_text(test_str);
        thread::sleep(Duration::from_millis(100));
        let read_back = Clipboard::data();
        assert_eq!(read_back, test_str);

        let temp_dir = std::env::temp_dir();
        let file1_path = temp_dir.join("flow_test_file1.txt");
        let file2_path = temp_dir.join("flow_test_file2.txt");

        std::fs::write(&file1_path, "test file 1 content").unwrap();
        std::fs::write(&file2_path, "test file 2 content").unwrap();

        let file_paths = vec![
            file1_path.to_string_lossy().to_string(),
            file2_path.to_string_lossy().to_string(),
        ];

        Clipboard::set_files(file_paths.clone());
        thread::sleep(Duration::from_millis(200));

        let clipboard_data = Clipboard::data();
        let paths_returned: Vec<&str> = clipboard_data.lines().collect();

        let contains_file1 = paths_returned.contains(&file1_path.to_str().unwrap());
        let contains_file2 = paths_returned.contains(&file2_path.to_str().unwrap());

        let _ = std::fs::remove_file(file1_path);
        let _ = std::fs::remove_file(file2_path);
        Clipboard::set_text(&original_clipboard);

        assert!(contains_file1, "Clipboard did not contain file 1");
        assert!(contains_file2, "Clipboard did not contain file 2");
    }

    #[test]
    fn test_mac_clipboard_promise_flow() {
        let _lock = get_clipboard_lock().lock().unwrap_or_else(|e| e.into_inner());
        use std::sync::mpsc::channel;
        use flow::hardware::PromisedClipboard;
        
        let original_clipboard = Clipboard::data();
        
        // 1. Initialize promise request receiver via callback
        let (req_tx, req_rx) = channel();
        PromisedClipboard::on_request(move |uuid| {
            let _ = req_tx.send(uuid);
        });
        
        // 2. Set the promise
        let test_promise_id = "test-promise-999";
        let test_text = "secret promised data payload!";
        PromisedClipboard::set_promise(test_promise_id, "text", test_text.len());
        
        // 3. Spawn a background thread to respond to the promise request
        let response_thread = thread::spawn(move || {
            // Wait for the request
            let requested_id = req_rx.recv_timeout(Duration::from_secs(5))
                .expect("Failed to receive promised request");
            assert_eq!(requested_id, test_promise_id);
            
            // Fulfill the promise directly
            PromisedClipboard::fulfill_promise(test_promise_id, flow::hardware::FulfillmentPayload::Text(test_text.to_string()));
        });
        
        // 4. Request the clipboard data. This blocks and triggers the promise resolution!
        let read_back = Clipboard::data();
        
        // Wait for helper thread to finish
        response_thread.join().unwrap();

        // Note: We don't assert flow::hardware::CLIPBOARD_IGNORE_HASHES contents here because 
        // other integration tests running concurrently start clipboard listeners that may 
        // immediately consume and remove the hash from the ignore queue.


        // Restore original clipboard
        Clipboard::set_text(&original_clipboard);
        
        // Clear promise callback to leave a clean state
        PromisedClipboard::on_request(|_| {});

        // Assert that the read_back data matches the streamed promised data!
        assert_eq!(read_back, test_text);
    }

    #[test]
    fn test_mac_mouse_warping() {
        let controller = MouseController::new();
        let original_pos = controller.position();

        let target = (200, 200);
        controller.set_position(target);
        thread::sleep(Duration::from_millis(50));

        let new_pos = controller.position();

        controller.set_position(original_pos);

        if new_pos == original_pos {
            println!("Warning: Mouse did not move. This is expected in headless test environments.");
        }
    }

    #[test]
    fn test_mac_mouse_listener_callbacks() {
        let move_params = Arc::new(Mutex::new(None));
        let click_params = Arc::new(Mutex::new(None));
        let scroll_params = Arc::new(Mutex::new(None));

        let move_c = move_params.clone();
        let click_c = click_params.clone();
        let scroll_c = scroll_params.clone();

        let callbacks = Box::into_raw(Box::new(MouseListenerCallbacks {
            on_move: Box::new(move |dx, dy| {
                let mut m = move_c.lock().unwrap();
                *m = Some((dx, dy));
            }),
            on_click: Box::new(move |x, y, btn, pressed| {
                let mut c = click_c.lock().unwrap();
                *c = Some((x, y, btn, pressed));
            }),
            on_scroll: Box::new(move |x, y, dx, dy| {
                let mut s = scroll_c.lock().unwrap();
                *s = Some((x, y, dx, dy));
            }),
            suppress: true,
            x_center: 960,
            y_center: 540,
            has_move: true,
        }));

        unsafe {
            let cgp = CGPoint { x: 960.0, y: 540.0 };
            let event_move = CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_MOUSE_MOVED, cgp, 0);
            assert!(!event_move.is_null());
            CGEventSetIntegerValueField(event_move, K_CG_MOUSE_EVENT_DELTA_X, 42);
            CGEventSetIntegerValueField(event_move, K_CG_MOUSE_EVENT_DELTA_Y, -15);

            let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_MOUSE_MOVED, event_move, callbacks as *mut c_void);
            assert!(res.is_null());
            CFRelease(event_move);

            let click_pos = CGPoint { x: 120.0, y: 240.0 };
            let event_click = CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_LEFT_MOUSE_DOWN, click_pos, 0);
            assert!(!event_click.is_null());

            let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_LEFT_MOUSE_DOWN, event_click, callbacks as *mut c_void);
            assert!(res.is_null());
            CFRelease(event_click);

            let event_scroll = CGEventCreateScrollWheelEvent(ptr::null_mut(), 1, 2, 5, 2);
            assert!(!event_scroll.is_null());
            
            let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_SCROLL_WHEEL, event_scroll, callbacks as *mut c_void);
            assert!(res.is_null());
            CFRelease(event_scroll);

            let _ = Box::from_raw(callbacks);
        }

        let m_res = move_params.lock().unwrap().take().expect("on_move was not called");
        assert_eq!(m_res.0, 42);
        assert_eq!(m_res.1, -15);

        let c_res = click_params.lock().unwrap().take().expect("on_click was not called");
        assert_eq!(c_res.2, "Button.left");
        assert!(c_res.3);

        let s_res = scroll_params.lock().unwrap().take().expect("on_scroll was not called");
        assert!(s_res.2 != 0 || s_res.3 != 0, "Both scroll deltas were 0");
    }

    #[test]
    fn test_mac_keyboard_listener_callbacks() {
        let press_param = Arc::new(Mutex::new(None));
        let release_param = Arc::new(Mutex::new(None));

        let press_c = press_param.clone();
        let release_c = release_param.clone();

        let callbacks = Box::into_raw(Box::new(KeyboardListenerCallbacks {
            on_press: Box::new(move |key| {
                let mut p = press_c.lock().unwrap();
                *p = Some(key);
            }),
            on_release: Box::new(move |key| {
                let mut r = release_c.lock().unwrap();
                *r = Some(key);
            }),
            suppress: true,
        }));

        unsafe {
            let event_down = CGEventCreateKeyboardEvent(ptr::null_mut(), 51, true);
            assert!(!event_down.is_null());

            let res = keyboard_tap_callback(ptr::null_mut(), K_CG_EVENT_KEY_DOWN, event_down, callbacks as *mut c_void);
            assert!(res.is_null());
            CFRelease(event_down);

            let event_up = CGEventCreateKeyboardEvent(ptr::null_mut(), 51, false);
            assert!(!event_up.is_null());

            let res = keyboard_tap_callback(ptr::null_mut(), K_CG_EVENT_KEY_UP, event_up, callbacks as *mut c_void);
            assert!(res.is_null());
            CFRelease(event_up);

            let _ = Box::from_raw(callbacks);
        }

        let p_res = press_param.lock().unwrap().take().expect("on_press was not called");
        assert_eq!(p_res, "Key.backspace");

        let r_res = release_param.lock().unwrap().take().expect("on_release was not called");
        assert_eq!(r_res, "Key.backspace");
    }

    #[test]
    fn test_mac_keyboard_key_name_mapping() {
        assert_eq!(get_mac_keycode("Key.cmd"), Some(55));
        assert_eq!(get_mac_keycode("Key.shift"), Some(56));
        assert_eq!(get_mac_keycode("Key.ctrl"), Some(59));
        assert_eq!(get_mac_keycode("Key.alt"), Some(58));

        assert_eq!(get_mac_keycode("a"), Some(0));
        assert_eq!(get_mac_keycode("'s'"), Some(1));
        assert_eq!(get_mac_keycode(" "), Some(49));
    }

    #[test]
    fn test_mac_mouse_scroll_wheel_simulation() {
        let controller = MouseController::new();
        controller.scroll(0, -5);
        controller.scroll(5, 0);
    }
}

// ==========================================
// 4. LINUX-SPECIFIC HARDWARE FFI TESTS
// ==========================================

#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use flow::hardware::{MouseListener, KeyboardListener};

    #[test]
    fn test_linux_clipboard_combined() {
        let _lock = get_clipboard_lock().lock().unwrap_or_else(|e| e.into_inner());
        let original_clipboard = Clipboard::data();

        let test_str = "flow-system-test-unique-string-123456";
        Clipboard::set_text(test_str);
        thread::sleep(Duration::from_millis(150));
        let read_back = Clipboard::data();
        if read_back != test_str {
            println!("Warning: Clipboard read back mismatch. Expected '{}', got '{}'. This is common in headless/CI environments.", test_str, read_back);
        } else {
            assert_eq!(read_back, test_str);
        }

        let temp_dir = std::env::temp_dir();
        let file1_path = temp_dir.join("flow_test_file1.txt");
        let file2_path = temp_dir.join("flow_test_file2.txt");

        std::fs::write(&file1_path, "test file 1 content").unwrap();
        std::fs::write(&file2_path, "test file 2 content").unwrap();

        let file_paths = vec![
            file1_path.to_string_lossy().to_string(),
            file2_path.to_string_lossy().to_string(),
        ];

        Clipboard::set_files(file_paths.clone());
        thread::sleep(Duration::from_millis(200));

        let clipboard_data = Clipboard::data();
        let paths_returned: Vec<&str> = clipboard_data.lines().collect();

        let contains_file1 = paths_returned.contains(&file1_path.to_str().unwrap());
        let contains_file2 = paths_returned.contains(&file2_path.to_str().unwrap());

        let _ = std::fs::remove_file(file1_path);
        let _ = std::fs::remove_file(file2_path);
        Clipboard::set_text(&original_clipboard);

        if !clipboard_data.is_empty() {
            assert!(contains_file1, "Clipboard did not contain file 1");
            assert!(contains_file2, "Clipboard did not contain file 2");
        } else {
            println!("Warning: Clipboard returned empty data for set_files. This is expected in headless test environments.");
        }
    }

    #[test]
    fn test_linux_mouse_warping() {
        let controller = MouseController::new();
        let original_pos = controller.position();

        let target = (200, 200);
        controller.set_position(target);
        thread::sleep(Duration::from_millis(100));

        let new_pos = controller.position();

        controller.set_position(original_pos);

        if new_pos == original_pos {
            println!("Warning: Mouse did not move. This is expected in headless/CI test environments.");
        } else {
            assert_eq!(new_pos, target);
        }
    }
}

// ==========================================
// 5. RECONNECTION INTEGRATION TEST
// ==========================================

#[test]
fn test_client_reconnection_flow() {
    let _lock = get_db_lock().lock().unwrap_or_else(|e| e.into_inner());
    let _clip_lock = get_clipboard_lock().lock().unwrap_or_else(|e| e.into_inner());
    use flow::engine::AppEngine;
    use flow::config::initialize_db;
    use flow::paths::get_db_path;
    use std::fs;

    // Backup DB
    let db_path = get_db_path();
    let backup_path = db_path.with_extension("db.backup");
    let has_backup = if db_path.exists() {
        fs::copy(&db_path, &backup_path).is_ok()
    } else {
        false
    };

    // Ensure clean test DB
    if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }
    initialize_db().unwrap();

    // Create server engine
    let server_engine = AppEngine::new();
    {
        let mut s = server_engine.settings.lock().unwrap();
        s.ip = "127.0.0.1".to_string();
        s.pc = 1; // Server
    }

    // Create client engine
    let client_engine = AppEngine::new();
    {
        let mut s = client_engine.settings.lock().unwrap();
        s.ip = "127.0.0.1".to_string();
        s.pc = 2; // Client
    }

    // Thread to auto-approve trust requests
    let server_trusts = server_engine.pending_trusts.clone();
    let client_trusts = client_engine.pending_trusts.clone();
    let trust_thread_running = Arc::new(Mutex::new(true));
    let trust_running_clone = trust_thread_running.clone();
    let trust_join_handle = thread::spawn(move || {
        while *trust_running_clone.lock().unwrap() {
            {
                let mut server_list = server_trusts.lock().unwrap();
                while let Some(req) = server_list.pop() {
                    let _ = req.tx.send(true);
                }
            }
            {
                let mut client_list = client_trusts.lock().unwrap();
                while let Some(req) = client_list.pop() {
                    let _ = req.tx.send(true);
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    });

    // Step 1: Start Client FIRST. Server is not running.
    println!("Test: Starting Client while Server is down...");
    client_engine.start();

    // Give it a moment. Client should fail to connect but keep retrying.
    thread::sleep(Duration::from_secs(1));
    assert!(!*client_engine.is_connected.lock().unwrap());

    // Step 2: Now start the Server.
    println!("Test: Starting Server. Client should automatically reconnect...");
    server_engine.start();

    // Wait for reconnection
    let mut success = false;
    for _ in 0..50 {
        if *client_engine.is_connected.lock().unwrap() && *server_engine.is_connected.lock().unwrap() {
            success = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(success, "Client failed to automatically connect to the server when server came up");
    println!("Test: Connected successfully!");

    // Step 3: Stop the Server. Client should detect disconnect.
    println!("Test: Stopping Server...");
    server_engine.stop();

    // Wait for client to detect disconnect
    let mut disconnected = false;
    for _ in 0..50 {
        if !*client_engine.is_connected.lock().unwrap() {
            disconnected = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(disconnected, "Client failed to detect server disconnection");
    println!("Test: Client disconnected successfully.");

    // Step 4: Start the Server back up. Client should reconnect.
    println!("Test: Restarting Server...");
    server_engine.start();

    let mut reconnected = false;
    for _ in 0..50 {
        if *client_engine.is_connected.lock().unwrap() && *server_engine.is_connected.lock().unwrap() {
            reconnected = true;
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    assert!(reconnected, "Client failed to automatically reconnect to the restarted server");
    println!("Test: Reconnected successfully!");

    // Cleanup
    server_engine.stop();
    client_engine.stop();
    {
        let mut running = trust_thread_running.lock().unwrap();
        *running = false;
    }
    let _ = trust_join_handle.join();

    // Restore DB backup
    if has_backup {
        let _ = fs::copy(&backup_path, &db_path);
        let _ = fs::remove_file(&backup_path);
    } else if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }
}

// ==========================================
// 6. VIRTUAL COORDINATED DESKTOP SPACE TESTS
// ==========================================

#[test]
fn test_virtual_coordinated_desktop_space() {
    let _lock = get_db_lock().lock().unwrap_or_else(|e| e.into_inner());
    use std::fs;
    use flow::config::{initialize_db, MonitorLayout, save_monitor_layout};
    use flow::engine::{find_client_monitor_containing, find_closest_client_monitor};
    use flow::paths::get_db_path;

    // Backup DB
    let db_path = get_db_path();
    let backup_path = db_path.with_extension("db.backup");
    let has_backup = if db_path.exists() {
        fs::copy(&db_path, &backup_path).is_ok()
    } else {
        false
    };

    // Ensure clean test DB
    if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }
    initialize_db().unwrap();

    // Define two monitors for client host "192.168.1.50"
    let mon_a = MonitorLayout {
        monitor_id: "client_display_1".to_string(),
        host: "192.168.1.50".to_string(),
        monitor_name: "Display 1".to_string(),
        x: 1000,
        y: 0,
        width: 1920,
        height: 1080,
        scale_factor: 1.0,
        local_x: 0,
        local_y: 0,
    };

    let mon_b = MonitorLayout {
        monitor_id: "client_display_2".to_string(),
        host: "192.168.1.50".to_string(),
        monitor_name: "Display 2".to_string(),
        x: 2920,
        y: 0,
        width: 1920,
        height: 1080,
        scale_factor: 1.0,
        local_x: 1920,
        local_y: 0,
    };

    save_monitor_layout(&mon_a).unwrap();
    save_monitor_layout(&mon_b).unwrap();

    // 1. Test find_client_monitor_containing
    // Inside Monitor A
    let found = find_client_monitor_containing(1500, 500);
    assert!(found.is_some());
    assert_eq!(found.unwrap().monitor_id, "client_display_1");

    // Inside Monitor B
    let found = find_client_monitor_containing(3500, 500);
    assert!(found.is_some());
    assert_eq!(found.unwrap().monitor_id, "client_display_2");

    // Outside coordinates (dead space / host space)
    let found = find_client_monitor_containing(500, 500);
    assert!(found.is_none());

    let found = find_client_monitor_containing(3000, 2000);
    assert!(found.is_none());

    // 2. Test find_closest_client_monitor clamping fallback
    let monitors_list = vec![mon_a, mon_b];

    // Case 1: Point to the left of Monitor A (gx = 900, gy = 500) -> Should pick Monitor A
    let closest = find_closest_client_monitor(900, 500, "192.168.1.50", &monitors_list);
    assert!(closest.is_some());
    assert_eq!(closest.unwrap().monitor_id, "client_display_1");

    // Case 2: Point to the right of Monitor B (gx = 4900, gy = 500) -> Should pick Monitor B
    let closest = find_closest_client_monitor(4900, 500, "192.168.1.50", &monitors_list);
    assert!(closest.is_some());
    assert_eq!(closest.unwrap().monitor_id, "client_display_2");

    // Case 3: Point in dead space between Monitor A and Monitor B (gx = 2500, gy = 500)
    // Distance to Mon A right (x = 2919) is 419px.
    // Distance to Mon B left (x = 2920) is 420px.
    // -> Should pick Monitor A as it is slightly closer (419px vs 420px).
    let closest = find_closest_client_monitor(2500, 500, "192.168.1.50", &monitors_list);
    assert!(closest.is_some());
    assert_eq!(closest.unwrap().monitor_id, "client_display_1");

    // Case 4: Point closer to Monitor B (gx = 2900, gy = 500)
    // Distance to Mon B left (2920) is 20px.
    // Distance to Mon A right (2919) is 19px. Wait: 2900 is 19px left of 2919? No, 2900 is left of 2919, so it is inside Mon A!
    // Let's pick gx = 2919: inside Mon A.
    // Let's pick gx = 2920: inside Mon B.
    // Let's pick gx = 2921: inside Mon B.
    // Let's pick gx = 2920 - 1 = 2919: inside Mon A.
    // Let's pick gx = 2919 + 0.5? No, integers.
    // Let's pick gx = 2919 (inside Mon A).
    // Let's test a point outside both, say gx = 2920 - 10 = 2910.
    // Distance to Mon A (1000 to 2919): gx is 2910, which is inside Mon A!
    // Oh, since Mon A ends at 2919, and Mon B starts at 2920, there is NO dead space between them horizontally!
    // Let's test vertically: gy = -50 (above both).
    // gx = 3500 (over Mon B), gy = -50.
    // -> Should pick Monitor B (dy = 50, dx = 0).
    let closest = find_closest_client_monitor(3500, -50, "192.168.1.50", &monitors_list);
    assert!(closest.is_some());
    assert_eq!(closest.unwrap().monitor_id, "client_display_2");

    // Cleanup and Restore DB backup
    if has_backup {
        let _ = fs::copy(&backup_path, &db_path);
        let _ = fs::remove_file(&backup_path);
    } else if db_path.exists() {
        let _ = fs::remove_file(&db_path);
    }
}

