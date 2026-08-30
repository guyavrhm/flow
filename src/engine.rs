pub mod layout;
pub mod tracker;
pub mod clipboard;

use crate::config::{SettingsData, get_settings};
use crate::hardware::{
    ClipboardController, KeyboardListener, MouseListener,
};
use clipboard::{hash_clipboard_data, stream_offered_data, handle_incoming_clipboard_chunk};

pub struct OfferedData {
    pub id: String,
    pub format: String,
    pub data: String,
}

pub struct ClipboardAccumulator {
    pub id: String,
    pub format: String,
    pub size: usize,
    pub buffer: Vec<u8>,
}

use crate::network::protocol::{ClipboardPayload, ScreenMetrics};
use crate::network::tcp::{TcpClient, TcpServer};
use crate::network::udp::{UdpClient, UdpServer};
use crate::network::tls::PendingTrustRequest;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct ClientInfo {
    pub ip: String,
    pub monitors: Vec<crate::network::protocol::MonitorInfo>,
    pub uses_physical_pixels: bool,
    pub udp_addr: Option<SocketAddr>,
    pub cryptor: crate::crypto::UdpCryptor,
    pub udp_seq: u64,
    pub connection_id: u64,
}

pub struct AppEngine {
    pub settings: Arc<Mutex<SettingsData>>,
    pub is_running: Arc<Mutex<bool>>,
    pub global_mouse_x: Arc<Mutex<i32>>,
    pub global_mouse_y: Arc<Mutex<i32>>,

    // Server resources
    tcp_server: Arc<TcpServer>,
    udp_server: Arc<Mutex<Option<UdpServer>>>,
    pub active_clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
    current_controlled: Arc<Mutex<String>>, // "main" or client IP
    mouse_listener: Arc<Mutex<Option<MouseListener>>>,
    keyboard_listener: Arc<Mutex<Option<KeyboardListener>>>,

    // Client resources
    tcp_client: Arc<TcpClient>,
    udp_client: Arc<UdpClient>,

    clipboard_listener: Arc<Mutex<Option<crate::hardware::ClipboardListener>>>,
    clipboard_accumulator: Arc<Mutex<Option<ClipboardAccumulator>>>,

    // Status signals for UI
    pub is_connected: Arc<Mutex<bool>>,
    pub pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>,
    pub offered_data: Arc<Mutex<Option<OfferedData>>>,
}

impl AppEngine {
    pub fn new() -> Self {
        Self {
            settings: Arc::new(Mutex::new(get_settings().unwrap_or(SettingsData {
                ip: "".to_string(),
                password: "".to_string(),
                pc: 1, // Server
                encryption: 0,
            }))),
            is_running: Arc::new(Mutex::new(false)),
            global_mouse_x: Arc::new(Mutex::new(0)),
            global_mouse_y: Arc::new(Mutex::new(0)),
            tcp_server: Arc::new(TcpServer::new()),
            udp_server: Arc::new(Mutex::new(None)),
            active_clients: Arc::new(Mutex::new(HashMap::new())),
            current_controlled: Arc::new(Mutex::new("main".to_string())),
            mouse_listener: Arc::new(Mutex::new(None)),
            keyboard_listener: Arc::new(Mutex::new(None)),
            tcp_client: Arc::new(TcpClient::new()),
            udp_client: Arc::new(UdpClient::new()),
            clipboard_listener: Arc::new(Mutex::new(None)),
            clipboard_accumulator: Arc::new(Mutex::new(None)),
            is_connected: Arc::new(Mutex::new(false)),
            pending_trusts: Arc::new(Mutex::new(Vec::new())),
            offered_data: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self) {
        let mut running = self.is_running.lock().unwrap();
        if *running {
            return;
        }
        *running = true;

        let settings = {
            let s = self.settings.lock().unwrap();
            s.clone()
        };

        log::info!("Starting AppEngine (mode: {})", if settings.pc == 1 { "Server" } else { "Client" });

        // Initialize promised clipboard request callback
        let tcp_server = self.tcp_server.clone();
        let tcp_client = self.tcp_client.clone();
        let settings_clone = settings.clone();

        crate::hardware::PromisedClipboard::on_request(move |uuid| {
            log::info!("Promise callback: Clipboard requested payload {}, sending Request", uuid);
            let payload = ClipboardPayload::Request { id: uuid };
            if settings_clone.pc == 1 {
                tcp_server.broadcast_clipboard(&payload, None);
            } else {
                let _ = tcp_client.send_clipboard(&payload);
            }
        });

        if settings.pc == 1 {
            // Start Server Mode
            self.start_server(settings);
        } else {
            // Start Client Mode
            self.start_client(settings);
        }
    }

    pub fn stop(&self) {
        log::info!("Stopping AppEngine...");
        let mut running = self.is_running.lock().unwrap();
        if !*running {
            return;
        }
        *running = false;

        self.tcp_server.stop();
        {
            let mut u_server = self.udp_server.lock().unwrap();
            *u_server = None;
        }
        self.tcp_client.stop();
        self.udp_client.stop();

        // Release listeners
        {
            let mut ml = self.mouse_listener.lock().unwrap();
            *ml = None;
        }
        {
            let mut kl = self.keyboard_listener.lock().unwrap();
            *kl = None;
        }
        {
            let mut cl = self.clipboard_listener.lock().unwrap();
            if let Some(listener) = cl.take() {
                listener.stop();
            }
        }

        let mut conn = self.is_connected.lock().unwrap();
        *conn = false;

        log::info!("AppEngine stopped");
    }

    pub fn reload(&self) {
        log::info!("Reloading AppEngine...");
        self.stop();
        // Load fresh settings from DB
        if let Ok(fresh) = get_settings() {
            let mut s = self.settings.lock().unwrap();
            *s = fresh;
        }
        // Give socket reuse a small delay
        thread::sleep(Duration::from_millis(200));
        self.start();
    }

    fn start_server(&self, settings: SettingsData) {
        log::info!("Starting Server Mode");

        // Initialize or update server monitors in DB with current display dimensions
        let existing_layouts = crate::config::get_all_monitor_layouts().unwrap_or_default();
        let local_mons = crate::hardware::get_monitors();
        for m in local_mons {
            let monitor_id = format!("main_{}", m.name);
            let (x, y) = existing_layouts
                .iter()
                .find(|l| l.monitor_id == monitor_id)
                .map(|l| (l.x, l.y))
                .unwrap_or((m.local_x, m.local_y));

            let layout = crate::config::MonitorLayout {
                monitor_id,
                host: "main".to_string(),
                monitor_name: m.name,
                x,
                y,
                width: m.width,
                height: m.height,
                scale_factor: m.scale_factor,
                local_x: m.local_x,
                local_y: m.local_y,
            };
            let _ = crate::config::save_monitor_layout(&layout);
        }

        let active_clients = self.active_clients.clone();
        let current_controlled = self.current_controlled.clone();
        let is_connected = self.is_connected.clone();

        // Initialize UDP Server
        let udp_server = match UdpServer::new() {
            Ok(u) => u,
            Err(e) => {
                log::error!("Failed to bind UDP server: {:?}", e);
                return;
            }
        };
        let udp_server_arc = Arc::new(udp_server);
        {
            let mut u_lock = self.udp_server.lock().unwrap();
            *u_lock = Some((*udp_server_arc).clone());
        }

        let udp_server_handshake = udp_server_arc.clone();
        let active_clients_handshake = active_clients.clone();

        // TCP callbacks
        let is_connected_conn = is_connected.clone();
        let on_connect = move |ip: String, metrics: ScreenMetrics, cryptor: crate::crypto::UdpCryptor, connection_id: u64| {
            log::info!("[SERVER] Client TCP connected: {} (conn_id: {}). Waiting for UDP verification...", ip, connection_id);
            crate::state::STATE_MANAGER.set_peer_state(&ip, crate::state::PeerState::UdpHandshaking);

            // Register client monitors in database
            for m in &metrics.monitors {
                let monitor_id = format!("{}_{}", ip, m.name);
                let mut existing_layout = None;
                if let Ok(layouts) = crate::config::get_all_monitor_layouts() {
                    existing_layout = layouts.into_iter().find(|l| l.monitor_id == monitor_id);
                }

                if let Some(mut lay) = existing_layout {
                    lay.width = m.width;
                    lay.height = m.height;
                    lay.scale_factor = m.scale_factor;
                    lay.local_x = m.local_x;
                    lay.local_y = m.local_y;
                    let _ = crate::config::save_monitor_layout(&lay);
                } else {
                    let max_x = if let Ok(layouts) = crate::config::get_all_monitor_layouts() {
                        layouts.iter().map(|l| l.x + l.width).max().unwrap_or(0)
                    } else {
                        0
                    };
                    let layout = crate::config::MonitorLayout {
                        monitor_id,
                        host: ip.clone(),
                        monitor_name: m.name.clone(),
                        x: max_x + 50,
                        y: 0,
                        width: m.width,
                        height: m.height,
                        scale_factor: m.scale_factor,
                        local_x: m.local_x,
                        local_y: m.local_y,
                    };
                    let _ = crate::config::save_monitor_layout(&layout);
                }
            }

            // Perform UDP Handshake in a background task
            let udp_server_task = udp_server_handshake.clone();
            let active_clients_task = active_clients_handshake.clone();
            let is_connected_task = is_connected_conn.clone();
            let client_ip = ip.clone();
            let cryptor_clone = cryptor.clone();

            thread::spawn(move || {
                match udp_server_task.listen_handshake(&client_ip, &cryptor_clone) {
                    Ok(udp_addr) => {
                        let mut clients = active_clients_task.lock().unwrap();
                        clients.insert(
                            client_ip.clone(),
                            ClientInfo {
                                ip: client_ip.clone(),
                                monitors: metrics.monitors.clone(),
                                uses_physical_pixels: metrics.uses_physical_pixels,
                                udp_addr: Some(udp_addr),
                                cryptor: cryptor_clone.clone(),
                                udp_seq: 0,
                                connection_id,
                            },
                        );

                        crate::state::STATE_MANAGER.set_peer_state(
                            &client_ip,
                            crate::state::PeerState::FullyConnected {
                                metrics: metrics.clone(),
                                udp_addr,
                                cryptor: cryptor_clone,
                                connection_id,
                            },
                        );

                        {
                            let mut conn = is_connected_task.lock().unwrap();
                            *conn = true;
                        }
                        log::info!("[SERVER] Client [{}] is FULLY CONNECTED (TCP + verified UDP)", client_ip);
                        crate::state::STATE_MANAGER.request_repaint();
                    }
                    Err(e) => {
                        log::error!("[SERVER] UDP handshake failed for client [{}]: {:?}", client_ip, e);
                        crate::state::STATE_MANAGER.set_peer_state(&client_ip, crate::state::PeerState::Disconnected);
                    }
                }
            });
        };

        let is_connected_disc = self.is_connected.clone();
        let active_clients_disc = active_clients.clone();
        let current_controlled_disc = current_controlled.clone();
        let clipboard_accumulator_disc = self.clipboard_accumulator.clone();

        let on_disconnect = move |ip: String, conn_id: u64| {
            log::info!("[SERVER] Client disconnected: {} (conn_id: {})", ip, conn_id);
            let mut clients = active_clients_disc.lock().unwrap();
            
            let should_remove = if let Some(info) = clients.get(&ip) {
                info.connection_id == conn_id
            } else {
                true // If it was never in clients (failed handshake), still clean it up
            };

            if should_remove {
                clients.remove(&ip);
                crate::state::STATE_MANAGER.remove_peer(&ip);

                if clients.is_empty() {
                    let mut conn = is_connected_disc.lock().unwrap();
                    *conn = false;
                }

                let mut curr = current_controlled_disc.lock().unwrap();
                if *curr == ip {
                    log::warn!("[SERVER] Active controlled client [{}] disconnected. Reverting control to Host!", ip);
                    *curr = "main".to_string();
                    crate::state::STATE_MANAGER.emergency_release();
                }

                // Clean up progress bar & accumulator
                crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);
                *clipboard_accumulator_disc.lock().unwrap() = None;
                crate::state::STATE_MANAGER.request_repaint();
            } else {
                log::info!("[SERVER] Ignoring disconnect for {} as a newer connection exists", ip);
            }
        };

        let tcp_server_clip = self.tcp_server.clone();
        let clipboard_accumulator_server = self.clipboard_accumulator.clone();
        let offered_data_recv = self.offered_data.clone();

        let on_clipboard_recv = move |payload: ClipboardPayload, from_ip: String| {
            let data_repr = match &payload {
                ClipboardPayload::Text { text } => text.clone(),
                ClipboardPayload::Files { files } => format!("files:{}", files.len()),
                ClipboardPayload::Offer { id, size, format } => format!("offer:{}:{}:{}", id, size, format),
                ClipboardPayload::Request { id } => format!("request:{}", id),
                ClipboardPayload::Chunk { id, chunk_index, is_last, data } => format!("chunk:{}:{}:{}:{}", id, chunk_index, is_last, data.len()),
            };
            log::info!("Server: Received clipboard update from client {}: {}", from_ip, data_repr);

            let is_set_payload = match &payload {
                ClipboardPayload::Text { .. } | ClipboardPayload::Files { .. } => true,
                _ => false,
            };

            if is_set_payload {
                crate::hardware::IN_SET_CLIPBOARD.store(true, std::sync::atomic::Ordering::Relaxed);
            }

            match &payload {
                ClipboardPayload::Text { text } => {
                    ClipboardController::set_text(text);
                }
                ClipboardPayload::Files { files } => {
                    let local_paths = crate::network::protocol::write_clipboard_files(files);
                    ClipboardController::set_files(local_paths);
                }
                ClipboardPayload::Offer { id, size, format } => {
                    crate::hardware::PromisedClipboard::set_promise(id, format, *size);
                }
                ClipboardPayload::Request { id } => {
                    // Forward request to other clients
                    tcp_server_clip.broadcast_clipboard(&payload, Some(&from_ip));
                    
                    // Check if Server owns it
                    if let Some(active_offered) = offered_data_recv.lock().unwrap().as_ref() {
                        if &active_offered.id == id {
                            let offered_data = active_offered.data.clone();
                            let format = active_offered.format.clone();
                            let id = id.clone();
                            let tcp_server_stream = tcp_server_clip.clone();
                            thread::spawn(move || {
                                stream_offered_data(&id, &format, &offered_data, move |chunk_payload| {
                                    tcp_server_stream.broadcast_clipboard(&chunk_payload, None);
                                });
                            });
                        }
                    }
                }
                ClipboardPayload::Chunk { id, chunk_index: _, is_last, data } => {
                    // Forward chunk to other clients
                    tcp_server_clip.broadcast_clipboard(&payload, Some(&from_ip));
                    
                    // Check if Server needs it
                    handle_incoming_clipboard_chunk(id, *is_last, data, &clipboard_accumulator_server);
                }
            }

            if is_set_payload {
                let read_back = ClipboardController::data();
                if !read_back.is_empty() && read_back != "unknown format" {
                    crate::hardware::push_ignore_hash(hash_clipboard_data(&read_back));
                }
                crate::hardware::IN_SET_CLIPBOARD.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        };

        // Start TCP Server
        if let Err(e) =
            self.tcp_server
                .start(settings, self.pending_trusts.clone(), on_connect, on_disconnect, on_clipboard_recv)
        {
            log::error!("TCP Server failed to start: {:?}", e);
            return;
        }

        // Start Server Core Loops (Edge tracking & Clipboard polling)
        self.spawn_server_edge_tracking();
        self.start_clipboard_monitoring();
    }

    fn spawn_server_edge_tracking(&self) {
        let tracker = tracker::ServerEdgeTracker::new(
            self.is_running.clone(),
            self.active_clients.clone(),
            self.current_controlled.clone(),
            self.mouse_listener.clone(),
            self.keyboard_listener.clone(),
            self.udp_server.clone(),
            self.global_mouse_x.clone(),
            self.global_mouse_y.clone(),
        );
        tracker.spawn();
    }

    fn start_clipboard_monitoring(&self) {
        let manager = clipboard::ClipboardSyncManager::new(
            self.settings.clone(),
            self.tcp_server.clone(),
            self.tcp_client.clone(),
            self.offered_data.clone(),
            self.clipboard_listener.clone(),
        );
        manager.start();
    }

    fn start_client(&self, settings: SettingsData) {
        log::info!("Starting Client Mode");

        let server_ip = settings.ip.clone();
        let is_connected = self.is_connected.clone();
        let udp_client = self.udp_client.clone();
        let tcp_client = self.tcp_client.clone();
        let ip = settings.ip.clone();
        let pending_trusts = self.pending_trusts.clone();
        let is_running_loop = self.is_running.clone();
        let clipboard_accumulator_client = self.clipboard_accumulator.clone();
        let offered_data_client = self.offered_data.clone();

        thread::spawn(move || {
            while *is_running_loop.lock().unwrap() {
                let connected = {
                    let conn = is_connected.lock().unwrap();
                    *conn
                };

                if !connected {
                    log::info!("TCP Client attempting to connect to server at {}...", ip);

                    // Recreate closures on each connection attempt
                    let is_connected_conn = is_connected.clone();
                    let udp_client_conn = udp_client.clone();
                    let server_ip_conn = server_ip.clone();
                    let on_connect = move |key: [u8; 32], salt: [u8; 4]| {
                        log::info!("[CLIENT] TCP connected to server. Initializing verified UDP handshake...");
                        crate::state::STATE_MANAGER.set_peer_state(&server_ip_conn, crate::state::PeerState::UdpHandshaking);

                        match udp_client_conn.start(&server_ip_conn, key, salt) {
                            Ok(()) => {
                                log::info!("[CLIENT] Fully connected and verified with server at {}!", server_ip_conn);
                                {
                                    let mut conn = is_connected_conn.lock().unwrap();
                                    *conn = true;
                                }
                                crate::state::STATE_MANAGER.set_peer_state(
                                    &server_ip_conn,
                                    crate::state::PeerState::FullyConnected {
                                        metrics: ScreenMetrics {
                                            width: 0,
                                            height: 0,
                                            monitors: Vec::new(),
                                            uses_physical_pixels: false,
                                        },
                                        udp_addr: "0.0.0.0:8118".parse().unwrap(),
                                        cryptor: crate::crypto::UdpCryptor::new(&key, salt),
                                        connection_id: 0,
                                    },
                                );
                                crate::state::STATE_MANAGER.request_repaint();
                            }
                            Err(e) => {
                                log::error!("[CLIENT] UDP handshake with server failed: {:?}", e);
                                {
                                    let mut conn = is_connected_conn.lock().unwrap();
                                    *conn = false;
                                }
                                crate::state::STATE_MANAGER.set_peer_state(&server_ip_conn, crate::state::PeerState::Disconnected);
                                crate::state::STATE_MANAGER.request_repaint();
                            }
                        }
                    };

                    let is_connected_disc = is_connected.clone();
                    let udp_client_disc = udp_client.clone();
                    let clipboard_accumulator_disc = clipboard_accumulator_client.clone();
                    let server_ip_disc = server_ip.clone();
                    let on_disconnect = move || {
                        log::info!("[CLIENT] Disconnected from server");
                        {
                            let mut conn = is_connected_disc.lock().unwrap();
                            *conn = false;
                        }
                        udp_client_disc.stop();
                        crate::state::STATE_MANAGER.remove_peer(&server_ip_disc);
                        crate::state::STATE_MANAGER.request_repaint();
                        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);
                        *clipboard_accumulator_disc.lock().unwrap() = None;
                    };

                    let tcp_client_stream = tcp_client.clone();
                    let clipboard_accumulator_conn = clipboard_accumulator_client.clone();
                    let offered_data_loop = offered_data_client.clone();
                    let on_clipboard_recv = move |payload: ClipboardPayload| {
                        let data_repr = match &payload {
                            ClipboardPayload::Text { text } => text.clone(),
                            ClipboardPayload::Files { files } => format!("files:{}", files.len()),
                            ClipboardPayload::Offer { id, size, format } => format!("offer:{}:{}:{}", id, size, format),
                            ClipboardPayload::Request { id } => format!("request:{}", id),
                            ClipboardPayload::Chunk { id, chunk_index, is_last, data } => format!("chunk:{}:{}:{}:{}", id, chunk_index, is_last, data.len()),
                        };
                        log::info!("Client: Received clipboard update from server: {}", data_repr);

                        let is_set_payload = match &payload {
                            ClipboardPayload::Text { .. } | ClipboardPayload::Files { .. } => true,
                            _ => false,
                        };

                        if is_set_payload {
                            crate::hardware::IN_SET_CLIPBOARD.store(true, std::sync::atomic::Ordering::Relaxed);
                        }

                        match &payload {
                            ClipboardPayload::Text { text } => {
                                ClipboardController::set_text(text);
                            }
                            ClipboardPayload::Files { files } => {
                                let local_paths = crate::network::protocol::write_clipboard_files(files);
                                ClipboardController::set_files(local_paths);
                            }
                            ClipboardPayload::Offer { id, size, format } => {
                                crate::hardware::PromisedClipboard::set_promise(id, format, *size);
                            }
                            ClipboardPayload::Request { id } => {
                                if let Some(active_offered) = offered_data_loop.lock().unwrap().as_ref() {
                                    if &active_offered.id == id {
                                        let offered_data = active_offered.data.clone();
                                        let format = active_offered.format.clone();
                                        let id = id.clone();
                                        let tcp_client_stream_thread = tcp_client_stream.clone();
                                        thread::spawn(move || {
                                            stream_offered_data(&id, &format, &offered_data, move |chunk_payload| {
                                                let _ = tcp_client_stream_thread.send_clipboard(&chunk_payload);
                                            });
                                        });
                                    }
                                }
                            }
                            ClipboardPayload::Chunk { id, chunk_index: _, is_last, data } => {
                                handle_incoming_clipboard_chunk(id, *is_last, data, &clipboard_accumulator_conn);
                            }
                        }

                        if is_set_payload {
                            let read_back = ClipboardController::data();
                            if !read_back.is_empty() && read_back != "unknown format" {
                                crate::hardware::push_ignore_hash(hash_clipboard_data(&read_back));
                            }
                            crate::hardware::IN_SET_CLIPBOARD.store(false, std::sync::atomic::Ordering::Relaxed);
                        }
                    };

                    if let Err(e) = tcp_client.connect(
                        &ip,
                        settings.clone(),
                        pending_trusts.clone(),
                        on_connect,
                        on_disconnect,
                        on_clipboard_recv,
                    ) {
                        log::error!("TCP Client failed to connect: {:?}", e);
                    }
                }

                // Sleep for up to 2 seconds, but exit quickly if the app is stopped
                for _ in 0..10 {
                    if !*is_running_loop.lock().unwrap() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(200));
                }
            }
        });

        self.start_clipboard_monitoring();
    }
}



// Extracted to tracker and clipboard submodules


#[cfg(test)]
mod tests {
    use super::clipboard::hash_clipboard_data;

    #[test]
    fn test_hash_clipboard_data_determinism() {
        let text = "hello flow clipboard!";
        let h1 = hash_clipboard_data(text);
        let h2 = hash_clipboard_data(text);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_clipboard_data_different_inputs() {
        let text1 = "hello flow clipboard!";
        let text2 = "hello flow clipboard!!";
        let h1 = hash_clipboard_data(text1);
        let h2 = hash_clipboard_data(text2);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_clipboard_loopback_global_ignore_matching() {
        let sample_text = "test global loopback prevention payload";
        let data_hash = hash_clipboard_data(sample_text);
        
        // Push the hash to simulate an incoming payload completion
        crate::hardware::push_ignore_hash(data_hash);
        
        // Simulating the check that would be done in ClipboardListener callback
        let is_from_network = crate::hardware::check_and_consume_ignore_hash(data_hash);
        
        assert!(is_from_network, "Hash should be recognized as originating from the network via global ignore queue");
        
        // Verify that the hash was removed after matching
        let checked_again = crate::hardware::check_and_consume_ignore_hash(data_hash);
        assert!(!checked_again, "Hash should be removed from global ignore list after matching");
    }
}
