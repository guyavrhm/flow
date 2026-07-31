use crate::config::{ScreenAttachments, SettingsData, get_attachments, get_settings};
use crate::hardware::{
    Clipboard, KeyboardListener, MouseController, MouseListener, get_screeninfo,
};
use once_cell::sync::Lazy;

pub struct OfferedData {
    pub id: String,
    pub format: String,
    pub data: String,
}

pub static ACTIVE_OFFERED_DATA: Lazy<Mutex<Option<OfferedData>>> = Lazy::new(|| Mutex::new(None));

pub struct ClipboardAccumulator {
    pub id: String,
    pub format: String,
    pub size: usize,
    pub buffer: Vec<u8>,
}

use crate::network::protocol::{ClipboardPayload, InputEvent, ScreenMetrics};
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
    pub width: i32,
    pub height: i32,
    pub mouse_x: i32,
    pub mouse_y: i32,
    pub attachments: ScreenAttachments,
    pub udp_addr: Option<SocketAddr>,
    pub cryptor: crate::crypto::UdpCryptor,
    pub udp_seq: u64,
    pub connection_id: u64,
}

pub struct AppEngine {
    pub settings: Arc<Mutex<SettingsData>>,
    pub is_running: Arc<Mutex<bool>>,

    // Server resources
    tcp_server: Arc<TcpServer>,
    udp_server: Arc<Mutex<Option<UdpServer>>>,
    active_clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
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
        let on_connect = move |ip: String, metrics: ScreenMetrics, cryptor: crate::crypto::UdpCryptor, connection_id: u64| {
            log::info!("Server: Client connected: {} (conn_id: {})", ip, connection_id);
            {
                let mut conn = is_connected.lock().unwrap();
                *conn = true;
            }

            // Perform UDP Handshake in a background task
            let udp_server_task = udp_server_handshake.clone();
            let active_clients_task = active_clients_handshake.clone();
            let client_ip = ip.clone();
            let cryptor_clone = cryptor.clone();

            thread::spawn(move || {
                if let Ok(udp_addr) = udp_server_task.listen_handshake(&client_ip, &cryptor_clone) {
                    let mut clients = active_clients_task.lock().unwrap();
                    let attachments =
                        get_attachments(&client_ip).unwrap_or_else(|_| ScreenAttachments {
                            address: client_ip.clone(),
                            top: None,
                            right: None,
                            bottom: None,
                            left: None,
                        });

                    clients.insert(
                        client_ip.clone(),
                        ClientInfo {
                            ip: client_ip,
                            width: metrics.width,
                            height: metrics.height,
                            mouse_x: metrics.width / 2,
                            mouse_y: metrics.height / 2,
                            attachments,
                            udp_addr: Some(udp_addr),
                            cryptor: cryptor_clone,
                            udp_seq: 0,
                            connection_id,
                        },
                    );
                }
            });
        };

        let is_connected_disc = self.is_connected.clone();
        let active_clients_disc = active_clients.clone();
        let current_controlled_disc = current_controlled.clone();
        let mouse_listener_disc = self.mouse_listener.clone();
        let keyboard_listener_disc = self.keyboard_listener.clone();
        let clipboard_accumulator_disc = self.clipboard_accumulator.clone();

        let on_disconnect = move |ip: String, conn_id: u64| {
            log::info!("Server: Client disconnected: {} (conn_id: {})", ip, conn_id);
            let mut clients = active_clients_disc.lock().unwrap();
            
            let should_remove = if let Some(info) = clients.get(&ip) {
                info.connection_id == conn_id
            } else {
                false
            };

            if should_remove {
                clients.remove(&ip);

                if clients.is_empty() {
                    let mut conn = is_connected_disc.lock().unwrap();
                    *conn = false;
                }

                let mut curr = current_controlled_disc.lock().unwrap();
                if *curr == ip {
                    // Revert control to Server
                    *curr = "main".to_string();
                    let mut ml = mouse_listener_disc.lock().unwrap();
                    *ml = None;
                    let mut kl = keyboard_listener_disc.lock().unwrap();
                    *kl = None;
                }

                // Clean up progress bar & accumulator
                crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);
                *clipboard_accumulator_disc.lock().unwrap() = None;
            } else {
                log::info!("Server: Ignoring disconnect for {} as a newer connection exists", ip);
            }
        };

        let tcp_server_clip = self.tcp_server.clone();
        let clipboard_accumulator_server = self.clipboard_accumulator.clone();

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
                    Clipboard::set_text(text);
                }
                ClipboardPayload::Files { files } => {
                    let local_paths = crate::network::protocol::write_clipboard_files(files);
                    Clipboard::set_files(local_paths);
                }
                ClipboardPayload::Offer { id, size, format } => {
                    crate::hardware::PromisedClipboard::set_promise(id, format, *size);
                }
                ClipboardPayload::Request { id } => {
                    // Forward request to other clients
                    tcp_server_clip.broadcast_clipboard(&payload, Some(&from_ip));
                    
                    // Check if Server owns it
                    if let Some(active_offered) = ACTIVE_OFFERED_DATA.lock().unwrap().as_ref() {
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
                let read_back = Clipboard::data();
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
        let is_running = self.is_running.clone();
        let active_clients = self.active_clients.clone();
        let current_controlled = self.current_controlled.clone();
        let mouse_listener = self.mouse_listener.clone();
        let keyboard_listener = self.keyboard_listener.clone();
        let udp_server = self.udp_server.clone();

        thread::spawn(move || {
            let mouse_ctrl = MouseController::new();
            let server_metrics = get_screeninfo();

            while *is_running.lock().unwrap() {
                thread::sleep(Duration::from_millis(10));

                let is_main = {
                    let curr = current_controlled.lock().unwrap();
                    *curr == "main"
                };

                let mut next_controlled: Option<String> = None;
                let mut enter_pos = (0, 0);

                if is_main {
                    // Track local mouse coordinates
                    let pos = mouse_ctrl.position();
                    let main_attachments =
                        get_attachments("main").unwrap_or_else(|_| ScreenAttachments {
                            address: "main".to_string(),
                            top: None,
                            right: None,
                            bottom: None,
                            left: None,
                        });

                    let mut target_screen: Option<String> = None;
                    let mut side = 0; // 0=left, 1=right, 2=top, 3=bottom

                    if pos.0 < 5 {
                        target_screen = main_attachments.left.clone();
                        side = 0;
                    } else if pos.0 > server_metrics.0 - 5 {
                        target_screen = main_attachments.right.clone();
                        side = 1;
                    } else if pos.1 < 5 {
                        target_screen = main_attachments.top.clone();
                        side = 2;
                    } else if pos.1 > server_metrics.1 - 5 {
                        target_screen = main_attachments.bottom.clone();
                        side = 3;
                    }

                    if let Some(target) = target_screen {
                        let clients = active_clients.lock().unwrap();
                        if let Some(client) = clients.get(&target) {
                            next_controlled = Some(target.clone());
                            let ratio = if side == 0 || side == 1 {
                                server_metrics.1 as f64 / (pos.1 as f64 + 0.1)
                            } else {
                                server_metrics.0 as f64 / (pos.0 as f64 + 0.1)
                            };

                            enter_pos = match side {
                                1 => (8, (client.height as f64 / ratio) as i32),
                                0 => (client.width - 8, (client.height as f64 / ratio) as i32),
                                3 => ((client.width as f64 / ratio) as i32, 8),
                                _ => ((client.width as f64 / ratio) as i32, client.height - 8),
                            };
                        }
                    }
                }

                if let Some(ref target) = next_controlled {
                    log::info!("Edge reached! Transferring control from main to {}", target);
                    {
                        let mut curr = current_controlled.lock().unwrap();
                        *curr = target.clone();
                    }

                    {
                        let mut clients = active_clients.lock().unwrap();
                        if let Some(c) = clients.get_mut(target) {
                            c.mouse_x = enter_pos.0;
                            c.mouse_y = enter_pos.1;
                        }
                    }

                    let active_clients_cb = active_clients.clone();
                    let current_controlled_cb = current_controlled.clone();
                    let udp_server_cb = udp_server.clone();

                    let on_move = move |dx: i32, dy: i32| {
                        let curr = current_controlled_cb.lock().unwrap().clone();
                        let mut next_target: Option<String> = None;
                        let mut side = -1;
                        let mut client_width = 0;
                        let mut client_height = 0;
                        let mut client_mouse_x = 0;
                        let mut client_mouse_y = 0;

                        {
                            let mut clients = active_clients_cb.lock().unwrap();
                            if let Some(c) = clients.get_mut(&curr) {
                                c.mouse_x = (c.mouse_x + dx).clamp(0, c.width);
                                c.mouse_y = (c.mouse_y + dy).clamp(0, c.height);

                                if let Some(udp) = udp_server_cb.lock().unwrap().as_ref() {
                                    if let Some(addr) = c.udp_addr {
                                        c.udp_seq += 1;
                                        let seq = c.udp_seq;
                                        let _ = udp.send_event(
                                            &InputEvent::Move {
                                                x: c.mouse_x,
                                                y: c.mouse_y,
                                            },
                                            addr,
                                            &c.cryptor,
                                            seq,
                                        );
                                    }
                                }

                                if c.mouse_x < 5 {
                                    next_target = c.attachments.left.clone();
                                    side = 0;
                                } else if c.mouse_x > c.width - 5 {
                                    next_target = c.attachments.right.clone();
                                    side = 1;
                                } else if c.mouse_y < 5 {
                                    next_target = c.attachments.top.clone();
                                    side = 2;
                                } else if c.mouse_y > c.height - 5 {
                                    next_target = c.attachments.bottom.clone();
                                    side = 3;
                                }
                                client_width = c.width;
                                client_height = c.height;
                                client_mouse_x = c.mouse_x;
                                client_mouse_y = c.mouse_y;
                            }
                        }

                        if let Some(ref target) = next_target {
                            handle_client_edge_transition(
                                target,
                                side,
                                client_width,
                                client_height,
                                client_mouse_x,
                                client_mouse_y,
                                &current_controlled_cb,
                                &active_clients_cb,
                                &udp_server_cb,
                            );
                        }
                    };

                    let udp_server_click = udp_server.clone();
                    let active_clients_click = active_clients.clone();
                    let current_controlled_click = current_controlled.clone();

                    let on_click = move |_x: i32, _y: i32, btn: String, pressed: bool| {
                        let curr = current_controlled_click.lock().unwrap().clone();
                        let mut clients = active_clients_click.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_click.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::MouseClick {
                                            button: btn,
                                            pressed,
                                        },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    };

                    let udp_server_scroll = udp_server.clone();
                    let active_clients_scroll = active_clients.clone();
                    let current_controlled_scroll = current_controlled.clone();

                    let on_scroll = move |_x: i32, _y: i32, dx: i32, dy: i32| {
                        let curr = current_controlled_scroll.lock().unwrap().clone();
                        let mut clients = active_clients_scroll.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_scroll.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::MouseScroll { dx, dy },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    };

                    let ml = MouseListener::new(on_move, on_click, on_scroll, true);
                    ml.start();
                    log::debug!("Server: Initialized and started local mouse event listener.");
                    {
                        let mut ml_lock = mouse_listener.lock().unwrap();
                        *ml_lock = Some(ml);
                    }

                    let udp_server_press = udp_server.clone();
                    let active_clients_press = active_clients.clone();
                    let current_controlled_press = current_controlled.clone();

                    let on_press = move |key: String| {
                        let curr = current_controlled_press.lock().unwrap().clone();
                        let mut clients = active_clients_press.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_press.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::KeyPress { key, pressed: true },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    };

                    let udp_server_release = udp_server.clone();
                    let active_clients_release = active_clients.clone();
                    let current_controlled_release = current_controlled.clone();

                    let on_release = move |key: String| {
                        let curr = current_controlled_release.lock().unwrap().clone();
                        let mut clients = active_clients_release.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_release.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::KeyPress {
                                            key,
                                            pressed: false,
                                        },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    };

                    let kl = KeyboardListener::new(on_press, on_release, true);
                    kl.start();
                    log::debug!("Server: Initialized and started local keyboard event listener.");
                    {
                        let mut kl_lock = keyboard_listener.lock().unwrap();
                        *kl_lock = Some(kl);
                    }
                }
            }
        });
    }

    fn start_clipboard_monitoring(&self) {
        let settings = {
            let s = self.settings.lock().unwrap();
            s.clone()
        };
        let tcp_server = self.tcp_server.clone();
        let tcp_client = self.tcp_client.clone();

        let on_change = move || {
            let data = Clipboard::data();
            if data.is_empty() || data == "unknown format" {
                return;
            }

            let mut is_files = false;
            let mut total_size = 0;
            if data.starts_with("file://") || data.starts_with('/') {
                is_files = true;
                for path_str in data.lines() {
                    let path_str = path_str.trim_start_matches("file://");
                    let path = std::path::Path::new(path_str);
                    if path.exists() {
                        if path.is_file() {
                            if let Ok(metadata) = std::fs::metadata(path) {
                                total_size += metadata.len() as usize;
                            }
                        } else if path.is_dir() {
                            for entry in walkdir::WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
                                if entry.path().is_file() {
                                    if let Ok(metadata) = std::fs::metadata(entry.path()) {
                                        total_size += metadata.len() as usize;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                total_size = data.len();
            }

            let data_hash = hash_clipboard_data(&data);
            let is_from_network = crate::hardware::check_and_consume_ignore_hash(data_hash);

            if !is_from_network {
                if total_size > 5 * 1024 * 1024 {
                    let id = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        .to_string();
                    let format = if is_files { "files".to_string() } else { "text".to_string() };
                    log::info!("Clipboard: Local content is large ({} bytes). Registering promise id: {}", total_size, id);

                    {
                        let mut offered = ACTIVE_OFFERED_DATA.lock().unwrap();
                        *offered = Some(OfferedData {
                            id: id.clone(),
                            format: format.clone(),
                            data: data.clone(),
                        });
                    }

                    let payload = ClipboardPayload::Offer {
                        id,
                        size: total_size,
                        format,
                    };
                    if settings.pc == 1 {
                        tcp_server.broadcast_clipboard(&payload, None);
                    } else {
                        let _ = tcp_client.send_clipboard(&payload);
                    }
                } else {
                    log::info!("Clipboard: Local content updated, syncing...");
                    let payload = if is_files {
                        format_clipboard_data(&data)
                    } else {
                        Some(ClipboardPayload::Text { text: data })
                    };

                    if let Some(p) = payload {
                        if settings.pc == 1 {
                            tcp_server.broadcast_clipboard(&p, None);
                        } else {
                            let _ = tcp_client.send_clipboard(&p);
                        }
                    }
                }
            }
        };

        let listener = crate::hardware::ClipboardListener::new(on_change);
        listener.start();

        let mut lock = self.clipboard_listener.lock().unwrap();
        *lock = Some(listener);
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
                        log::info!("Client: Connected to server");
                        {
                            let mut conn = is_connected_conn.lock().unwrap();
                            *conn = true;
                        }
                        let _ = udp_client_conn.start(&server_ip_conn, key, salt);
                    };

                    let is_connected_disc = is_connected.clone();
                    let udp_client_disc = udp_client.clone();
                    let clipboard_accumulator_disc = clipboard_accumulator_client.clone();
                    let on_disconnect = move || {
                        log::info!("Client: Disconnected from server");
                        {
                            let mut conn = is_connected_disc.lock().unwrap();
                            *conn = false;
                        }
                        udp_client_disc.stop();
                        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);
                        *clipboard_accumulator_disc.lock().unwrap() = None;
                    };

                    let tcp_client_stream = tcp_client.clone();
                    let clipboard_accumulator_conn = clipboard_accumulator_client.clone();
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
                                Clipboard::set_text(text);
                            }
                            ClipboardPayload::Files { files } => {
                                let local_paths = crate::network::protocol::write_clipboard_files(files);
                                Clipboard::set_files(local_paths);
                            }
                            ClipboardPayload::Offer { id, size, format } => {
                                crate::hardware::PromisedClipboard::set_promise(id, format, *size);
                            }
                            ClipboardPayload::Request { id } => {
                                if let Some(active_offered) = ACTIVE_OFFERED_DATA.lock().unwrap().as_ref() {
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
                            let read_back = Clipboard::data();
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



pub fn handle_client_edge_transition(
    target: &str,
    side: i32, // 0=left, 1=right, 2=top, 3=bottom
    client_width: i32,
    client_height: i32,
    client_x: i32,
    client_y: i32,
    current_controlled: &Arc<Mutex<String>>,
    active_clients: &Arc<Mutex<HashMap<String, ClientInfo>>>,
    udp_server: &Arc<Mutex<Option<UdpServer>>>,
) {
    let server_metrics = get_screeninfo();

    let ratio = if side == 0 || side == 1 {
        client_height as f64 / (client_y as f64 + 0.1)
    } else {
        client_width as f64 / (client_x as f64 + 0.1)
    };

    if target == "main" {
        log::info!("Edge reached on Client! Transferring control to Server");

        let enter_pos = match side {
            1 => (8, (server_metrics.1 as f64 / ratio) as i32),
            0 => (
                server_metrics.0 - 8,
                (server_metrics.1 as f64 / ratio) as i32,
            ),
            3 => ((server_metrics.0 as f64 / ratio) as i32, 8),
            _ => (
                (server_metrics.0 as f64 / ratio) as i32,
                server_metrics.1 - 8,
            ),
        };

        let old_client = current_controlled.lock().unwrap().clone();
        {
            let mut clients = active_clients.lock().unwrap();
            if let Some(c) = clients.get_mut(&old_client) {
                if let Some(udp) = udp_server.lock().unwrap().as_ref() {
                    if let Some(addr) = c.udp_addr {
                        c.udp_seq += 1;
                        let seq = c.udp_seq;
                        let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, seq);
                    }
                }
            }
        }

        {
            let mut curr = current_controlled.lock().unwrap();
            *curr = "main".to_string();
        }

        let mouse = MouseController::new();
        mouse.set_position(enter_pos);
    } else {
        let mut clients = active_clients.lock().unwrap();
        if let Some(next_client) = clients.get(target) {
            log::info!(
                "Edge reached on Client! Transferring control directly to client: {}",
                target
            );

            let enter_pos = match side {
                1 => (8, (next_client.height as f64 / ratio) as i32),
                0 => (
                    next_client.width - 8,
                    (next_client.height as f64 / ratio) as i32,
                ),
                3 => ((next_client.width as f64 / ratio) as i32, 8),
                _ => (
                    (next_client.width as f64 / ratio) as i32,
                    next_client.height - 8,
                ),
            };

            let old_client = current_controlled.lock().unwrap().clone();
            if let Some(c) = clients.get_mut(&old_client) {
                if let Some(udp) = udp_server.lock().unwrap().as_ref() {
                    if let Some(addr) = c.udp_addr {
                        c.udp_seq += 1;
                        let seq = c.udp_seq;
                        let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, seq);
                    }
                }
            }

            if let Some(c) = clients.get_mut(target) {
                c.mouse_x = enter_pos.0;
                c.mouse_y = enter_pos.1;
            }

            drop(clients);
            {
                let mut curr = current_controlled.lock().unwrap();
                *curr = target.to_string();
            }
        } else {
            log::warn!("Edge transition target client {} not found in active clients list", target);
        }
    }
}

fn format_clipboard_data(paths_str: &str) -> Option<ClipboardPayload> {
    let paths: Vec<&str> = paths_str.lines().filter(|line| !line.is_empty()).collect();
    if paths.is_empty() {
        return None;
    }

    let mut files = Vec::new();
    let max_file_size = 50 * 1024 * 1024; // 50 MB

    let first_path = std::path::Path::new(paths[0]);
    let root_dir = match first_path.parent() {
        Some(p) => p,
        None => return None,
    };

    for path_str in paths {
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            log::warn!("Clipboard: Path does not exist: {:?}", path);
            continue;
        }

        if path.is_file() {
            if let Ok(metadata) = std::fs::metadata(path) {
                if metadata.len() > max_file_size {
                    log::warn!("Clipboard: Skipping {:?} (size {} bytes exceeds max limit of 50 MB)", path, metadata.len());
                    continue;
                }
            }
            match std::fs::read(path) {
                Ok(data) => {
                    if let Ok(rel) = path.strip_prefix(root_dir) {
                        files.push(crate::network::protocol::ClipboardFile {
                            is_dir: false,
                            name: rel.to_string_lossy().to_string().replace('\\', "/"),
                            data: Some(data),
                        });
                    }
                }
                Err(e) => {
                    log::error!("Clipboard: Failed to read file {:?}: {:?}", path, e);
                }
            }
        } else if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(root_dir) {
                files.push(crate::network::protocol::ClipboardFile {
                    is_dir: true,
                    name: rel.to_string_lossy().to_string().replace('\\', "/"),
                    data: None,
                });
            }

            for entry in walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let entry_path = entry.path();
                if entry_path == path {
                    continue;
                }
                if let Ok(rel) = entry_path.strip_prefix(root_dir) {
                    let rel_name = rel.to_string_lossy().to_string().replace('\\', "/");
                    if entry_path.is_file() {
                        if let Ok(metadata) = std::fs::metadata(entry_path) {
                            if metadata.len() > max_file_size {
                                log::warn!("Clipboard: Skipping nested file {:?} (size {} bytes exceeds max limit of 50 MB)", entry_path, metadata.len());
                                continue;
                            }
                        }
                        match std::fs::read(entry_path) {
                            Ok(data) => {
                                files.push(crate::network::protocol::ClipboardFile {
                                    is_dir: false,
                                    name: rel_name,
                                    data: Some(data),
                                });
                            }
                            Err(e) => {
                                log::error!("Clipboard: Failed to read nested file {:?}: {:?}", entry_path, e);
                            }
                        }
                    } else if entry_path.is_dir() {
                        files.push(crate::network::protocol::ClipboardFile {
                            is_dir: true,
                            name: rel_name,
                            data: None,
                        });
                    }
                }
            }
        }
    }

    if files.is_empty() {
        None
    } else {
        log::debug!("Clipboard: Formatted {} clipboard files to payload", files.len());
        Some(ClipboardPayload::Files { files })
    }
}



fn hash_clipboard_data(data: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    data.hash(&mut hasher);
    hasher.finish()
}

fn stream_offered_data<F>(id: &str, format: &str, data: &str, mut send_chunk: F)
where
    F: FnMut(ClipboardPayload),
{
    log::info!("Streaming clipboard data for promise id: {}, format: {}", id, format);
    let bytes = if format == "text" {
        data.as_bytes().to_vec()
    } else {
        if let Some(ClipboardPayload::Files { files }) = format_clipboard_data(data) {
            match serde_json::to_vec(&files) {
                Ok(b) => b,
                Err(e) => {
                    log::error!("Failed to serialize files payload for promise streaming: {:?}", e);
                    return;
                }
            }
        } else {
            log::warn!("No files to stream for format files, data: {}", data);
            return;
        }
    };

    let total_len = bytes.len();
    let chunk_size = 64 * 1024; // 64 KB
    let mut chunk_index = 0;
    let mut offset = 0;

    while offset < total_len {
        let end = std::cmp::min(offset + chunk_size, total_len);
        let chunk_data = bytes[offset..end].to_vec();
        offset = end;
        let is_last = offset >= total_len;

        let payload = ClipboardPayload::Chunk {
            id: id.to_string(),
            chunk_index,
            is_last,
            data: chunk_data,
        };

        send_chunk(payload);
        chunk_index += 1;

        thread::sleep(Duration::from_millis(5));
    }
}

fn handle_incoming_clipboard_chunk(
    id: &str,
    is_last: bool,
    data: &[u8],
    accumulator: &Arc<Mutex<Option<ClipboardAccumulator>>>,
) {
    let has_tx = {
        let active_lock = crate::hardware::ACTIVE_PROMISE.lock().unwrap();
        active_lock.as_ref()
            .filter(|p| p.uuid == id)
            .map(|p| p.tx.is_some())
            .unwrap_or(false)
    };

    if !has_tx {
        return;
    }

    let mut accum_lock = accumulator.lock().unwrap();

    if accum_lock.is_none() || accum_lock.as_ref().map(|a| &a.id) != Some(&id.to_string()) {
        let format = crate::hardware::get_active_promise_format(id).unwrap_or_else(|| "text".to_string());
        let size = crate::hardware::ACTIVE_PROMISE.lock().unwrap().as_ref()
            .filter(|p| p.uuid == id)
            .map(|p| p.size)
            .unwrap_or(0);

        *accum_lock = Some(ClipboardAccumulator {
            id: id.to_string(),
            format,
            size,
            buffer: Vec::new(),
        });

        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(1, std::sync::atomic::Ordering::Relaxed);
    }

    let mut is_done = false;
    let mut format = String::new();
    let mut buffer = Vec::new();

    if let Some(ref mut accum) = *accum_lock {
        accum.buffer.extend_from_slice(data);
        if accum.size > 0 {
            let percent = (accum.buffer.len() * 100) / accum.size;
            let percent = std::cmp::min(100, percent as u32);
            crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(percent, std::sync::atomic::Ordering::Relaxed);
        }
        if is_last {
            is_done = true;
            format = accum.format.clone();
            buffer = std::mem::take(&mut accum.buffer);
        }
    }

    if is_done {
        *accum_lock = None;
        crate::hardware::CLIPBOARD_SYNC_PROGRESS.store(0, std::sync::atomic::Ordering::Relaxed);

        let payload = match format.as_str() {
            "text" => {
                if let Ok(text) = String::from_utf8(buffer) {
                    Some(crate::hardware::FulfillmentPayload::Text(text))
                } else {
                    log::error!("Clipboard: Failed to decode text payload UTF-8 string");
                    None
                }
            }
            "files" => {
                if let Ok(files) = serde_json::from_slice::<Vec<crate::network::protocol::ClipboardFile>>(&buffer) {
                    let local_paths = crate::network::protocol::write_clipboard_files(&files);
                    Some(crate::hardware::FulfillmentPayload::Files(local_paths))
                } else {
                    log::error!("Clipboard: Failed to deserialize files payload from JSON");
                    None
                }
            }
            _ => None,
        };

        if let Some(p) = payload {
            crate::hardware::PromisedClipboard::fulfill_promise(id, p);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

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
