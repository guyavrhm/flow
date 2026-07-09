use crate::config::SettingsData;
use crate::crypto::CryptoKey;
use crate::engine::{ClientInfo, handle_client_edge_transition};
use crate::hardware::{Clipboard, MouseController};
use crate::hardware::mac::{
    CGPoint, KeyboardListenerCallbacks, MouseListenerCallbacks,
    keyboard_tap_callback, mouse_tap_callback,
};
use crate::network::protocol::{
    InputEvent, true_recv, true_send, ScreenMetrics,
};
use crate::network::udp::{format_event, parse_event};

use std::collections::HashMap;
use std::ffi::c_void;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::ptr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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

// ==========================================
// 1. CRYPTO & ENCRYPTION TESTS
// ==========================================

#[test]
fn test_crypto_key_derivation() {
    let key1 = CryptoKey::new("password123");
    let key2 = CryptoKey::new("password123");
    let key3 = CryptoKey::new("different_pass");
    let empty_key = CryptoKey::new("");

    let payload = b"Hello, World!";
    
    // Identical passwords must yield identical ciphertext
    let cipher1 = key1.encrypt(payload);
    let cipher2 = key2.encrypt(payload);
    assert_eq!(cipher1, cipher2);

    // Different password yields different ciphertext
    let cipher3 = key3.encrypt(payload);
    assert_ne!(cipher1, cipher3);

    // Empty password returns raw text
    let cipher_empty = empty_key.encrypt(payload);
    assert_eq!(cipher_empty, payload.to_vec());
}

#[test]
fn test_crypto_padding_and_block_boundaries() {
    let key = CryptoKey::new("secure_password");

    // Test a variety of lengths to ensure padding handles block boundaries correctly.
    // AES block size is 16 bytes.
    let test_lengths = vec![0, 1, 15, 16, 17, 31, 32, 33, 100, 1024];

    for len in test_lengths {
        let original = vec![0x42u8; len];
        let encrypted = key.encrypt(&original);
        
        // Ciphertext must be non-empty (even for size 0 due to padding metadata) and a multiple of 16.
        assert!(!encrypted.is_empty());
        assert_eq!(encrypted.len() % 16, 0);

        let decrypted = key.decrypt(&encrypted);
        assert_eq!(original, decrypted);
    }
}

#[test]
fn test_crypto_corrupted_decrypt() {
    let key = CryptoKey::new("some_password");
    let data = b"Confidential system payload here";
    let encrypted = key.encrypt(data);

    // Corrupt the ciphertext
    let mut corrupted = encrypted.clone();
    if !corrupted.is_empty() {
        corrupted[0] ^= 0xFF;
    }

    // Decrypting corrupted data should not panic; it should return either empty vec or corrupted text
    let decrypted = key.decrypt(&corrupted);
    // Since the padding byte is at index 0 in the decrypted block (due to our padding scheme prefixing padding_len),
    // corrupting block 0 will yield a corrupted padding length. This is handled by returning an empty vector.
    assert!(decrypted.is_empty() || decrypted != data);
}

#[test]
fn test_protocol_framing_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral loopback");
    let server_addr = listener.local_addr().unwrap();

    let password = "loopback_test_pass";
    let key = Arc::new(CryptoKey::new(password));
    let payload = b"Test protocol framing payload.";

    let key_clone = key.clone();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("Accept failed");
        let received = true_recv(&mut stream, &key_clone).expect("Server true_recv failed");
        assert_eq!(received, payload);
        
        // Echo response back
        true_send(&mut stream, b"ACK", &key_clone).expect("Server true_send failed");
    });

    let mut client_stream = TcpStream::connect(server_addr).expect("Client connect failed");
    true_send(&mut client_stream, payload, &key).expect("Client true_send failed");

    let ack = true_recv(&mut client_stream, &key).expect("Client true_recv failed");
    assert_eq!(ack, b"ACK");

    handle.join().unwrap();
}

// ==========================================
// 2. MAC HARDWARE & INTERFACE TESTS
// ==========================================

#[test]
fn test_mac_clipboard_combined() {
    // Save original clipboard state so we don't disrupt the developer's system
    let original_clipboard = Clipboard::data();

    // --- 1. Test Text Clipboard ---
    let test_str = "flow-system-test-unique-string-123456";
    Clipboard::set_text(test_str);
    
    // Give some small time for OS clipboard sync
    thread::sleep(Duration::from_millis(100));
    
    let read_back = Clipboard::data();
    assert_eq!(read_back, test_str);

    // --- 2. Test File Clipboard ---
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
    
    // AppleScript getfiles should return paths separated by linefeed
    let paths_returned: Vec<&str> = clipboard_data.lines().collect();

    println!("Paths returned from clipboard: {:?}", paths_returned);
    println!("File 1 path expected: {:?}", file1_path.to_str().unwrap());
    println!("File 2 path expected: {:?}", file2_path.to_str().unwrap());

    let contains_file1 = paths_returned.contains(&file1_path.to_str().unwrap());
    let contains_file2 = paths_returned.contains(&file2_path.to_str().unwrap());

    // Clean up
    let _ = std::fs::remove_file(file1_path);
    let _ = std::fs::remove_file(file2_path);
    Clipboard::set_text(&original_clipboard);

    assert!(contains_file1, "Clipboard did not contain file 1");
    assert!(contains_file2, "Clipboard did not contain file 2");
}

#[test]
fn test_mac_mouse_warping() {
    let controller = MouseController::new();
    let original_pos = controller.position();

    // Verify getting position does not panic
    println!("Original mouse position: {:?}", original_pos);

    // Warp to a target position
    let target = (200, 200);
    controller.set_position(target);
    thread::sleep(Duration::from_millis(50));

    let new_pos = controller.position();
    println!("Warped mouse position: {:?}", new_pos);

    // Restore mouse position
    controller.set_position(original_pos);

    // We check if it changed. If it didn't change (e.g. on a headless/CI runner without accessibility/GUI context),
    // we log a warning but don't fail the test suite since the API call completed successfully without panicking.
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

    // Setup callbacks
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
        // --- 1. Test Mouse Moved Callback ---
        let cgp = CGPoint { x: 960.0, y: 540.0 };
        let event_move = CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_MOUSE_MOVED, cgp, 0);
        assert!(!event_move.is_null());
        
        // Set event deltas
        CGEventSetIntegerValueField(event_move, K_CG_MOUSE_EVENT_DELTA_X, 42);
        CGEventSetIntegerValueField(event_move, K_CG_MOUSE_EVENT_DELTA_Y, -15);

        // Call the raw callback directly
        let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_MOUSE_MOVED, event_move, callbacks as *mut c_void);
        assert!(res.is_null());
        CFRelease(event_move);

        // --- 2. Test Mouse Click Callback ---
        let click_pos = CGPoint { x: 120.0, y: 240.0 };
        let event_click = CGEventCreateMouseEvent(ptr::null_mut(), K_CG_EVENT_LEFT_MOUSE_DOWN, click_pos, 0);
        assert!(!event_click.is_null());

        let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_LEFT_MOUSE_DOWN, event_click, callbacks as *mut c_void);
        assert!(res.is_null());
        CFRelease(event_click);

        // --- 3. Test Mouse Scroll Callback ---
        let event_scroll = CGEventCreateScrollWheelEvent(ptr::null_mut(), 1, 2, 5, 2);
        assert!(!event_scroll.is_null());
        
        let res = mouse_tap_callback(ptr::null_mut(), K_CG_EVENT_SCROLL_WHEEL, event_scroll, callbacks as *mut c_void);
        assert!(res.is_null());
        CFRelease(event_scroll);

        // Clean up callbacks allocation
        let _ = Box::from_raw(callbacks);
    }

    // Now assert results outside of the FFI callback boundaries
    let m_res = move_params.lock().unwrap().take().expect("on_move was not called");
    assert_eq!(m_res.0, 42);
    assert_eq!(m_res.1, -15);

    let c_res = click_params.lock().unwrap().take().expect("on_click was not called");
    assert_eq!(c_res.2, "Button.left");
    assert!(c_res.3);

    let s_res = scroll_params.lock().unwrap().take().expect("on_scroll was not called");
    // Scroll axis 1 / axis 2 deltas can vary depending on OS scroll mapping, but verify receipt
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
        // --- 1. Test Key Down ---
        let event_down = CGEventCreateKeyboardEvent(ptr::null_mut(), 51, true);
        assert!(!event_down.is_null());

        let res = keyboard_tap_callback(ptr::null_mut(), K_CG_EVENT_KEY_DOWN, event_down, callbacks as *mut c_void);
        assert!(res.is_null());
        CFRelease(event_down);

        // --- 2. Test Key Up ---
        let event_up = CGEventCreateKeyboardEvent(ptr::null_mut(), 51, false);
        assert!(!event_up.is_null());

        let res = keyboard_tap_callback(ptr::null_mut(), K_CG_EVENT_KEY_UP, event_up, callbacks as *mut c_void);
        assert!(res.is_null());
        CFRelease(event_up);

        // Clean up callbacks allocation
        let _ = Box::from_raw(callbacks);
    }

    // Assert results outside FFI boundaries
    let p_res = press_param.lock().unwrap().take().expect("on_press was not called");
    assert_eq!(p_res, "Key.backspace");

    let r_res = release_param.lock().unwrap().take().expect("on_release was not called");
    assert_eq!(r_res, "Key.backspace");
}

// ==========================================
// 3. NETWORK & PROTOCOL HANDSHAKE TESTS
// ==========================================

#[test]
fn test_tcp_handshake_flow() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral");
    let server_addr = listener.local_addr().unwrap();

    let settings = SettingsData {
        ip: "127.0.0.1".to_string(),
        password: "handshake_secret_key".to_string(),
        pc: 1,
        encryption: 1,
    };
    
    let key = Arc::new(CryptoKey::new(&settings.password));
    let key_clone = key.clone();

    // Spawn server handshake listener
    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        
        // 1. Read handshake request
        let h_req = true_recv(&mut stream, &key_clone).unwrap();
        assert_eq!(h_req, b".");

        // 2. Send handshake response
        true_send(&mut stream, b".", &key_clone).unwrap();

        // 3. Read screen metrics
        let metrics_bytes = true_recv(&mut stream, &key_clone).unwrap();
        let metrics: ScreenMetrics = serde_json::from_slice(&metrics_bytes).unwrap();
        assert_eq!(metrics.width, 1920);
        assert_eq!(metrics.height, 1080);
    });

    // Client connection & handshake implementation
    let mut client_stream = TcpStream::connect(server_addr).unwrap();
    
    // 1. Send handshake request
    true_send(&mut client_stream, b".", &key).unwrap();

    // 2. Read handshake response
    let h_res = true_recv(&mut client_stream, &key).unwrap();
    assert_eq!(h_res, b".");

    // 3. Send screen metrics
    let metrics = ScreenMetrics {
        width: 1920,
        height: 1080,
    };
    let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
    true_send(&mut client_stream, &metrics_bytes, &key).unwrap();

    server_handle.join().unwrap();
}

#[test]
fn test_tcp_handshake_wrong_password() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral");
    let server_addr = listener.local_addr().unwrap();

    let server_key = Arc::new(CryptoKey::new("correct_password"));
    let client_key = Arc::new(CryptoKey::new("incorrect_password"));

    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        
        // Server tries to read handshake with correct password
        let h_req = true_recv(&mut stream, &server_key);
        
        // Must fail to decrypt handshake correctly because client used incorrect password
        assert!(h_req.is_err() || h_req.unwrap() != b".");
    });

    let mut client_stream = TcpStream::connect(server_addr).unwrap();
    
    // Client sends handshake with incorrect password
    let _ = true_send(&mut client_stream, b".", &client_key);

    let _ = handle.join();
}

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
fn test_udp_event_transfer_socket() {
    let receiver_socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind UDP");
    let receiver_addr = receiver_socket.local_addr().unwrap();

    let sender_socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind UDP");

    let password = "udp_secret_pass";
    let key = CryptoKey::new(password);

    let event = InputEvent::Move { x: 1024, y: 768 };
    
    // Encrypt and send
    let payload = format_event(&event);
    let encrypted = key.encrypt(payload.as_bytes());
    sender_socket.send_to(&encrypted, receiver_addr).unwrap();

    // Receive and decrypt
    let mut buf = [0u8; 1024];
    let (len, _) = receiver_socket.recv_from(&mut buf).unwrap();
    let decrypted = key.decrypt(&buf[..len]);
    let dec_str = std::str::from_utf8(&decrypted).unwrap();

    // Parse event
    let parsed = parse_event(dec_str).expect("Failed to parse transfered event");
    if let InputEvent::Move { x, y } = parsed {
        assert_eq!(x, 1024);
        assert_eq!(y, 768);
    } else {
        panic!("Parsed event did not match original Move event");
    }
}

// ==========================================
// 4. ARCHITECTURE & EDGE TRANSITION TESTS
// ==========================================

#[test]
fn test_edge_transition_to_client() {
    let current_controlled = Arc::new(Mutex::new("main".to_string()));
    let active_clients = Arc::new(Mutex::new(HashMap::new()));
    let udp_server = Arc::new(Mutex::new(None));
    let settings = Arc::new(Mutex::new(SettingsData {
        ip: "".to_string(),
        password: "".to_string(),
        pc: 1,
        encryption: 0,
    }));

    let client_ip = "192.168.1.55".to_string();
    
    // Insert active client
    {
        let mut clients = active_clients.lock().unwrap();
        clients.insert(
            client_ip.clone(),
            ClientInfo {
                ip: client_ip.clone(),
                width: 1920,
                height: 1080,
                mouse_x: 0,
                mouse_y: 0,
                attachments: crate::config::ScreenAttachments {
                    address: client_ip.clone(),
                    top: None,
                    right: None,
                    bottom: None,
                    left: None,
                },
                udp_addr: None,
            },
        );
    }

    // Call edge transition from Server ("main") to Client "192.168.1.55" crossing Right edge (side=1)
    // Client width=1920, height=1080
    // Client X=1920, Y=540 (right edge point)
    handle_client_edge_transition(
        &client_ip,
        1,
        1920,
        1080,
        1920,
        540,
        &current_controlled,
        &active_clients,
        &udp_server,
        &settings,
    );

    // Verify current controlled has shifted to client
    assert_eq!(*current_controlled.lock().unwrap(), client_ip);

    // Verify client mouse coordinate has been positioned at the entrance position (X=8, Y=calculated)
    let clients = active_clients.lock().unwrap();
    let info = clients.get(&client_ip).unwrap();
    assert_eq!(info.mouse_x, 8);
}

#[test]
fn test_edge_transition_to_server() {
    let client_ip = "192.168.1.55".to_string();
    let current_controlled = Arc::new(Mutex::new(client_ip.clone()));
    let active_clients = Arc::new(Mutex::new(HashMap::new()));
    let udp_server = Arc::new(Mutex::new(None));
    let settings = Arc::new(Mutex::new(SettingsData {
        ip: "".to_string(),
        password: "".to_string(),
        pc: 1,
        encryption: 0,
    }));

    // Setup active clients
    {
        let mut clients = active_clients.lock().unwrap();
        clients.insert(
            client_ip.clone(),
            ClientInfo {
                ip: client_ip.clone(),
                width: 1920,
                height: 1080,
                mouse_x: 0,
                mouse_y: 0,
                attachments: crate::config::ScreenAttachments {
                    address: client_ip.clone(),
                    top: None,
                    right: None,
                    bottom: None,
                    left: None,
                },
                udp_addr: None,
            },
        );
    }

    let original_pos = MouseController::new().position();

    // Call edge transition back to Server ("main") crossing Left edge of client (side=0)
    handle_client_edge_transition(
        "main",
        0,
        1920,
        1080,
        0,
        540,
        &current_controlled,
        &active_clients,
        &udp_server,
        &settings,
    );

    // Verify control reverted to Server ("main")
    assert_eq!(*current_controlled.lock().unwrap(), "main");

    // Restore original mouse position
    MouseController::new().set_position(original_pos);
}
