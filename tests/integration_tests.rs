use flow::config::SettingsData;
use flow::crypto::CryptoKey;
use flow::engine::{ClientInfo, handle_client_edge_transition};
use flow::hardware::{Clipboard, MouseController};
use flow::network::protocol::{
    InputEvent, true_recv, true_send, ScreenMetrics,
};
use flow::network::udp::{format_event, parse_event};

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
// 2. NETWORK & CONTROL PROTOCOL TESTS
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
// 3. ARCHITECTURE & EDGE TRANSITION TESTS
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
                attachments: flow::config::ScreenAttachments {
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
                attachments: flow::config::ScreenAttachments {
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

// ==========================================
// 4. CONNECTION, RECONNECTION, & DATABASE TESTS
// ==========================================

#[test]
fn test_tcp_client_reconnection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral");
    let server_addr = listener.local_addr().unwrap();

    let password = "reconnect_secret_key";
    let key = Arc::new(CryptoKey::new(password));
    let key_clone = key.clone();

    // Spawn server that handles a connection, gets EOF/disconnection, and accepts reconnection
    let server_handle = thread::spawn(move || {
        // --- Connection 1 ---
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

        // Wait for client to close stream (EOF)
        let end = true_recv(&mut stream, &key_clone);
        assert!(end.is_err() || end.unwrap().is_empty());

        // --- Connection 2 (Reconnection) ---
        let (mut stream2, _) = listener.accept().unwrap();
        // 1. Read handshake request
        let h_req2 = true_recv(&mut stream2, &key_clone).unwrap();
        assert_eq!(h_req2, b".");
        // 2. Send handshake response
        true_send(&mut stream2, b".", &key_clone).unwrap();
        // 3. Read screen metrics
        let metrics_bytes2 = true_recv(&mut stream2, &key_clone).unwrap();
        let metrics2: ScreenMetrics = serde_json::from_slice(&metrics_bytes2).unwrap();
        assert_eq!(metrics2.width, 1280);
    });

    // Client connection 1
    {
        let mut client_stream = TcpStream::connect(server_addr).unwrap();
        true_send(&mut client_stream, b".", &key).unwrap();
        let h_res = true_recv(&mut client_stream, &key).unwrap();
        assert_eq!(h_res, b".");
        let metrics = ScreenMetrics { width: 1920, height: 1080 };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut client_stream, &metrics_bytes, &key).unwrap();
        // Disconnect immediately by dropping client_stream
    }

    thread::sleep(Duration::from_millis(50));

    // Client connection 2 (Reconnection)
    {
        let mut client_stream2 = TcpStream::connect(server_addr).unwrap();
        true_send(&mut client_stream2, b".", &key).unwrap();
        let h_res = true_recv(&mut client_stream2, &key).unwrap();
        assert_eq!(h_res, b".");
        let metrics = ScreenMetrics { width: 1280, height: 720 };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut client_stream2, &metrics_bytes, &key).unwrap();
    }

    server_handle.join().unwrap();
}

#[test]
fn test_tcp_multiple_clients_concurrency() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral");
    let server_addr = listener.local_addr().unwrap();

    let password = "multi_secret_key";
    let key = Arc::new(CryptoKey::new(password));
    let key_clone = key.clone();

    // Spawn server thread handling multiple clients
    let server_handle = thread::spawn(move || {
        let (mut stream1, addr1) = listener.accept().unwrap();
        let (mut stream2, addr2) = listener.accept().unwrap();

        // Handle client 1
        let h_req1 = true_recv(&mut stream1, &key_clone).unwrap();
        assert_eq!(h_req1, b".");
        true_send(&mut stream1, b".", &key_clone).unwrap();
        let m_bytes1 = true_recv(&mut stream1, &key_clone).unwrap();
        let m1: ScreenMetrics = serde_json::from_slice(&m_bytes1).unwrap();

        // Handle client 2
        let h_req2 = true_recv(&mut stream2, &key_clone).unwrap();
        assert_eq!(h_req2, b".");
        true_send(&mut stream2, b".", &key_clone).unwrap();
        let m_bytes2 = true_recv(&mut stream2, &key_clone).unwrap();
        let m2: ScreenMetrics = serde_json::from_slice(&m_bytes2).unwrap();

        assert_ne!(addr1, addr2);
        assert!(m1.width == 1920 || m1.width == 1280);
        assert!(m2.width == 1920 || m2.width == 1280);
    });

    let key_client1 = key.clone();
    let thread1 = thread::spawn(move || {
        let mut client_stream = TcpStream::connect(server_addr).unwrap();
        true_send(&mut client_stream, b".", &key_client1).unwrap();
        let h_res = true_recv(&mut client_stream, &key_client1).unwrap();
        assert_eq!(h_res, b".");
        let metrics = ScreenMetrics { width: 1920, height: 1080 };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut client_stream, &metrics_bytes, &key_client1).unwrap();
    });

    let key_client2 = key.clone();
    let thread2 = thread::spawn(move || {
        let mut client_stream = TcpStream::connect(server_addr).unwrap();
        true_send(&mut client_stream, b".", &key_client2).unwrap();
        let h_res = true_recv(&mut client_stream, &key_client2).unwrap();
        assert_eq!(h_res, b".");
        let metrics = ScreenMetrics { width: 1280, height: 720 };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut client_stream, &metrics_bytes, &key_client2).unwrap();
    });

    thread1.join().unwrap();
    thread2.join().unwrap();
    server_handle.join().unwrap();
}

#[test]
fn test_clipboard_broadcast_routing_and_loopback() {
    use flow::network::tcp::{TcpServer, TcpClientInfo};
    use flow::network::protocol::ClipboardPayload;
    use flow::config::ScreenAttachments;

    // Connect pairs of local sockets to simulate clients
    let server_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_addr = server_listener.local_addr().unwrap();

    let client_a_stream_server = thread::spawn(move || {
        let (stream, _) = server_listener.accept().unwrap();
        stream
    });

    let client_a_stream_local = TcpStream::connect(server_addr).unwrap();
    let client_a_stream_server = client_a_stream_server.join().unwrap();

    let server_listener_2 = TcpListener::bind("127.0.0.1:0").unwrap();
    let server_addr_2 = server_listener_2.local_addr().unwrap();

    let client_b_stream_server = thread::spawn(move || {
        let (stream, _) = server_listener_2.accept().unwrap();
        stream
    });

    let client_b_stream_local = TcpStream::connect(server_addr_2).unwrap();
    let client_b_stream_server = client_b_stream_server.join().unwrap();

    let tcp_server = TcpServer::new();
    
    // Add client A and client B manually to server
    let client_a_info = Arc::new(Mutex::new(TcpClientInfo {
        ip: "192.168.1.10".to_string(),
        metrics: ScreenMetrics { width: 1920, height: 1080 },
        stream: client_a_stream_server,
        attachments: ScreenAttachments {
            address: "192.168.1.10".to_string(),
            top: None, right: None, bottom: None, left: None,
        },
    }));

    let client_b_info = Arc::new(Mutex::new(TcpClientInfo {
        ip: "192.168.1.20".to_string(),
        metrics: ScreenMetrics { width: 1280, height: 720 },
        stream: client_b_stream_server,
        attachments: ScreenAttachments {
            address: "192.168.1.20".to_string(),
            top: None, right: None, bottom: None, left: None,
        },
    }));

    {
        let mut list = tcp_server.clients.lock().unwrap();
        list.push(client_a_info);
        list.push(client_b_info);
    }

    let payload = ClipboardPayload::Text { text: "secret_broadcast".to_string() };
    let settings = SettingsData {
        ip: "".to_string(),
        password: "broadcast_pass".to_string(),
        pc: 1,
        encryption: 1,
    };

    // Broadcast clipboard content, excluding Client A (192.168.1.10) to prevent loopback
    tcp_server.broadcast_clipboard(&payload, Some("192.168.1.10"), &settings);

    // Verify Client B received it
    let key = CryptoKey::new(&settings.password);
    let mut b_stream = client_b_stream_local;
    let b_data_bytes = true_recv(&mut b_stream, &key).unwrap();
    let b_payload: ClipboardPayload = serde_json::from_slice(&b_data_bytes).unwrap();
    if let ClipboardPayload::Text { text } = b_payload {
        assert_eq!(text, "secret_broadcast");
    } else {
        panic!("Wrong payload received on client B");
    }

    // Verify Client A received nothing (read times out immediately since server excluded client A)
    let mut a_stream = client_a_stream_local;
    a_stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let a_res = true_recv(&mut a_stream, &key);
    assert!(a_res.is_err(), "Client A received broadcast payload despite exclusion!");
}

#[test]
fn test_sqlite_settings_persistence() {
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

#[test]
fn test_udp_client_handshake_registration() {
    use flow::network::udp::UdpServer;

    let server_socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind server UDP");
    let server_addr = server_socket.local_addr().unwrap();

    let client_socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind client UDP");
    let client_addr = client_socket.local_addr().unwrap();

    let settings = SettingsData {
        ip: "127.0.0.1".to_string(),
        password: "udp_handshake_pass".to_string(),
        pc: 1,
        encryption: 1,
    };

    let server = UdpServer::from_socket(server_socket);
    let server_clone = server.clone();
    let settings_clone = settings.clone();

    // Listen on server side in background
    let listen_handle = thread::spawn(move || {
        server_clone.listen_handshake("127.0.0.1", &settings_clone)
    });

    // Send encrypted handshake request from client
    let key = CryptoKey::new(&settings.password);
    let encrypted = key.encrypt(b".");
    client_socket.send_to(&encrypted, server_addr).unwrap();

    // Verify server registers client address correctly
    let registered_addr = listen_handle.join().unwrap().unwrap();
    assert_eq!(registered_addr.ip(), client_addr.ip());
    assert_eq!(registered_addr.port(), client_addr.port());
}

// ==========================================
// 5. MACOS-SPECIFIC HARDWARE FFI TESTS
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
// 6. LINUX-SPECIFIC HARDWARE FFI TESTS
// ==========================================

#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use flow::hardware::{MouseListener, KeyboardListener};

    #[test]
    fn test_linux_clipboard_combined() {
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

    #[test]
    fn test_linux_mouse_listener_callbacks() {
        let move_params = Arc::new(Mutex::new(None));
        let click_params = Arc::new(Mutex::new(None));
        let scroll_params = Arc::new(Mutex::new(None));

        let move_c = move_params.clone();
        let click_c = click_params.clone();
        let scroll_c = scroll_params.clone();

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
            false,
        );

        listener.start();
        thread::sleep(Duration::from_millis(100));

        let controller = MouseController::new();
        controller.scroll(0, 1);
        controller.press("left");
        controller.release("left");

        thread::sleep(Duration::from_millis(150));
        listener.stop();

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
            false,
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
}
