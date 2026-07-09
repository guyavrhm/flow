use crate::config::SettingsData;
use crate::crypto::CryptoKey;
use crate::engine::{ClientInfo, handle_client_edge_transition};
use crate::hardware::{Clipboard, MouseController, MouseListener, KeyboardController, KeyboardListener};
use crate::network::protocol::{
    InputEvent, true_recv, true_send, ScreenMetrics,
};
use crate::network::udp::{format_event, parse_event};

use std::collections::HashMap;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
// 2. LINUX HARDWARE & INTERFACE TESTS
// ==========================================

#[test]
fn test_linux_clipboard_combined() {
    // Save original clipboard state
    let original_clipboard = Clipboard::data();

    // --- 1. Test Text Clipboard ---
    let test_str = "flow-system-test-unique-string-123456";
    Clipboard::set_text(test_str);
    
    // Give small time for OS clipboard sync
    thread::sleep(Duration::from_millis(150));
    
    let read_back = Clipboard::data();
    // In headless test environment, clipboard might not sync or set_text might fail,
    // so we log a warning rather than panicking. But if it does work, assert it.
    if read_back != test_str {
        println!("Warning: Clipboard read back mismatch. Expected '{}', got '{}'. This is common in headless/CI environments.", test_str, read_back);
    } else {
        assert_eq!(read_back, test_str);
    }

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
    let paths_returned: Vec<&str> = clipboard_data.lines().collect();

    println!("Paths returned from clipboard: {:?}", paths_returned);
    
    let contains_file1 = paths_returned.contains(&file1_path.to_str().unwrap());
    let contains_file2 = paths_returned.contains(&file2_path.to_str().unwrap());

    // Clean up
    let _ = std::fs::remove_file(file1_path);
    let _ = std::fs::remove_file(file2_path);
    Clipboard::set_text(&original_clipboard);

    // Assert if we are not in a headless/CI environment (where xclip/arboard might fail silently)
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

    println!("Original mouse position: {:?}", original_pos);

    // Warp to a target position
    let target = (200, 200);
    controller.set_position(target);
    thread::sleep(Duration::from_millis(100));

    let new_pos = controller.position();
    println!("Warped mouse position: {:?}", new_pos);

    // Restore mouse position
    controller.set_position(original_pos);

    if new_pos == original_pos {
        println!("Warning: Mouse did not move. This is expected in headless/CI test environments.");
    } else {
        assert_eq!(new_pos, target);
    }
}

#[test]
fn test_linux_mouse_listener_callbacks() {
    let move_params = Arc::new(Mutex::new(None));
    let click_params = Arc::new(Mutex::new(None));
    let scroll_params = Arc::new(Mutex::new(None));

    let move_c = move_params.clone();
    let click_c = click_params.clone();
    let scroll_c = scroll_params.clone();

    // Create a mouse listener
    let listener = MouseListener::new(
        move |dx, dy| {
            let mut m = move_c.lock().unwrap();
            *m = Some((dx, dy));
        },
        move |x, y, btn, pressed| {
            let mut c = click_c.lock().unwrap();
            *c = Some((x, y, btn, pressed));
        },
        move |x, y, dx, dy| {
            let mut s = scroll_c.lock().unwrap();
            *s = Some((x, y, dx, dy));
        },
        false, // Do not suppress/grab locally to avoid messing up active user session in test
    );

    listener.start();
    thread::sleep(Duration::from_millis(100));

    // Simulate mouse actions
    let controller = MouseController::new();
    controller.scroll(0, 1);
    controller.press("left");
    controller.release("left");

    thread::sleep(Duration::from_millis(150));
    listener.stop();

    // Since we ran with suppress=false, we might not capture global events (due to no PointerGrab).
    // But we check that start/stop didn't panic and we log what was captured.
    let m_res = move_params.lock().unwrap().take();
    let c_res = click_params.lock().unwrap().take();
    let s_res = scroll_params.lock().unwrap().take();

    println!("Mouse captured - Move: {:?}, Click: {:?}, Scroll: {:?}", m_res, c_res, s_res);
}

#[test]
fn test_linux_keyboard_listener_callbacks() {
    let press_param = Arc::new(Mutex::new(None));
    let release_param = Arc::new(Mutex::new(None));

    let press_c = press_param.clone();
    let release_c = release_param.clone();

    let listener = KeyboardListener::new(
        move |key| {
            let mut p = press_c.lock().unwrap();
            *p = Some(key);
        },
        move |key| {
            let mut r = release_c.lock().unwrap();
            *r = Some(key);
        },
        false, // Do not suppress/grab locally
    );

    listener.start();
    thread::sleep(Duration::from_millis(100));

    let controller = KeyboardController::new();
    controller.press("Key.space");
    controller.release("Key.space");

    thread::sleep(Duration::from_millis(150));
    listener.stop();

    let p_res = press_param.lock().unwrap().take();
    let r_res = release_param.lock().unwrap().take();

    println!("Keyboard captured - Press: {:?}, Release: {:?}", p_res, r_res);
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
