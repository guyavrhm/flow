use crate::config::{ScreenAttachments, SettingsData, get_attachments, get_settings};
use crate::hardware::{
    Clipboard, KeyboardListener, MouseController, MouseListener, get_screeninfo,
};
use crate::network::protocol::{ClipboardPayload, InputEvent, ScreenMetrics};
use crate::network::tcp::{TcpClient, TcpServer};
use crate::network::udp::{UdpClient, UdpServer};
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

    // Clipboard loop history to prevent feedback loops
    clipboard_history: Arc<Mutex<Vec<String>>>,

    // Status signals for UI
    pub is_connected: Arc<Mutex<bool>>,
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
            clipboard_history: Arc::new(Mutex::new(Vec::new())),
            is_connected: Arc::new(Mutex::new(false)),
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
        let settings_clone = self.settings.clone();

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
        let settings_handshake = settings.clone();

        // TCP callbacks
        let on_connect = move |ip: String, metrics: ScreenMetrics| {
            log::info!("Server: Client connected: {}", ip);
            {
                let mut conn = is_connected.lock().unwrap();
                *conn = true;
            }

            // Perform UDP Handshake in a background task
            let udp_server_task = udp_server_handshake.clone();
            let active_clients_task = active_clients_handshake.clone();
            let settings_task = settings_handshake.clone();
            let client_ip = ip.clone();

            thread::spawn(move || {
                if let Ok(udp_addr) = udp_server_task.listen_handshake(&client_ip, &settings_task) {
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

        let on_disconnect = move |ip: String| {
            log::info!("Server: Client disconnected: {}", ip);
            let mut clients = active_clients_disc.lock().unwrap();
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
        };

        let clipboard_history = self.clipboard_history.clone();
        let tcp_server_clip = self.tcp_server.clone();
        let settings_clip = settings_clone.clone();

        let on_clipboard_recv = move |payload: ClipboardPayload, from_ip: String| {
            let data_repr = match &payload {
                ClipboardPayload::Text { text } => text.clone(),
                ClipboardPayload::Files { files } => format!("files:{}", files.len()),
            };
            log::info!("Server: Received clipboard update from client {}: {}", from_ip, data_repr);

            {
                let mut history = clipboard_history.lock().unwrap();
                history.push(data_repr.clone());
                if history.len() > 5 {
                    history.remove(0);
                }
            }

            match &payload {
                ClipboardPayload::Text { text } => {
                    Clipboard::set_text(text);
                }
                ClipboardPayload::Files { files } => {
                    let local_paths = write_clipboard_files(files);
                    Clipboard::set_files(local_paths);
                }
            }

            // Broadcast to other clients
            let s_data = {
                let s = settings_clip.lock().unwrap();
                s.clone()
            };
            tcp_server_clip.broadcast_clipboard(&payload, Some(&from_ip), &s_data);
        };

        // Start TCP Server
        if let Err(e) =
            self.tcp_server
                .start(settings, on_connect, on_disconnect, on_clipboard_recv)
        {
            log::error!("TCP Server failed to start: {:?}", e);
            return;
        }

        // Start Server Core Loops (Edge tracking & Clipboard polling)
        self.spawn_server_edge_tracking();
        self.spawn_server_clipboard_monitoring();
    }

    fn spawn_server_edge_tracking(&self) {
        let is_running = self.is_running.clone();
        let active_clients = self.active_clients.clone();
        let current_controlled = self.current_controlled.clone();
        let mouse_listener = self.mouse_listener.clone();
        let keyboard_listener = self.keyboard_listener.clone();
        let udp_server = self.udp_server.clone();
        let settings = self.settings.clone();

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
                    let settings_cb = settings.clone();

                    let on_move = move |dx: i32, dy: i32| {
                        let curr = current_controlled_cb.lock().unwrap().clone();
                        let mut clients = active_clients_cb.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            c.mouse_x = (c.mouse_x + dx).clamp(0, c.width);
                            c.mouse_y = (c.mouse_y + dy).clamp(0, c.height);

                            if let Some(udp) = udp_server_cb.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    let s_data = settings_cb.lock().unwrap().clone();
                                    let _ = udp.send_event(
                                        &InputEvent::Move {
                                            x: c.mouse_x,
                                            y: c.mouse_y,
                                        },
                                        addr,
                                        &s_data,
                                    );
                                }
                            }

                            let mut side = -1;
                            let mut next_target: Option<String> = None;

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

                            if let Some(ref target) = next_target {
                                handle_client_edge_transition(
                                    target,
                                    side,
                                    c.width,
                                    c.height,
                                    c.mouse_x,
                                    c.mouse_y,
                                    &current_controlled_cb,
                                    &active_clients_cb,
                                    &udp_server_cb,
                                    &settings_cb,
                                );
                            }
                        }
                    };

                    let udp_server_click = udp_server.clone();
                    let active_clients_click = active_clients.clone();
                    let current_controlled_click = current_controlled.clone();
                    let settings_click = settings.clone();

                    let on_click = move |_x: i32, _y: i32, btn: String, pressed: bool| {
                        let curr = current_controlled_click.lock().unwrap().clone();
                        let clients = active_clients_click.lock().unwrap();
                        if let Some(c) = clients.get(&curr) {
                            if let Some(udp) = udp_server_click.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    let s_data = settings_click.lock().unwrap().clone();
                                    let _ = udp.send_event(
                                        &InputEvent::MouseClick {
                                            button: btn,
                                            pressed,
                                        },
                                        addr,
                                        &s_data,
                                    );
                                }
                            }
                        }
                    };

                    let udp_server_scroll = udp_server.clone();
                    let active_clients_scroll = active_clients.clone();
                    let current_controlled_scroll = current_controlled.clone();
                    let settings_scroll = settings.clone();

                    let on_scroll = move |_x: i32, _y: i32, dx: i32, dy: i32| {
                        let curr = current_controlled_scroll.lock().unwrap().clone();
                        let clients = active_clients_scroll.lock().unwrap();
                        if let Some(c) = clients.get(&curr) {
                            if let Some(udp) = udp_server_scroll.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    let s_data = settings_scroll.lock().unwrap().clone();
                                    let _ = udp.send_event(
                                        &InputEvent::MouseScroll { dx, dy },
                                        addr,
                                        &s_data,
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
                    let settings_press = settings.clone();

                    let on_press = move |key: String| {
                        let curr = current_controlled_press.lock().unwrap().clone();
                        let clients = active_clients_press.lock().unwrap();
                        if let Some(c) = clients.get(&curr) {
                            if let Some(udp) = udp_server_press.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    let s_data = settings_press.lock().unwrap().clone();
                                    let _ = udp.send_event(
                                        &InputEvent::KeyPress { key, pressed: true },
                                        addr,
                                        &s_data,
                                    );
                                }
                            }
                        }
                    };

                    let udp_server_release = udp_server.clone();
                    let active_clients_release = active_clients.clone();
                    let current_controlled_release = current_controlled.clone();
                    let settings_release = settings.clone();

                    let on_release = move |key: String| {
                        let curr = current_controlled_release.lock().unwrap().clone();
                        let clients = active_clients_release.lock().unwrap();
                        if let Some(c) = clients.get(&curr) {
                            if let Some(udp) = udp_server_release.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    let s_data = settings_release.lock().unwrap().clone();
                                    let _ = udp.send_event(
                                        &InputEvent::KeyPress {
                                            key,
                                            pressed: false,
                                        },
                                        addr,
                                        &s_data,
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

    fn spawn_server_clipboard_monitoring(&self) {
        let is_running = self.is_running.clone();
        let clipboard_history = self.clipboard_history.clone();
        let tcp_server = self.tcp_server.clone();
        let settings = self.settings.clone();

        thread::spawn(move || {
            let mut recent_value = String::new();

            while *is_running.lock().unwrap() {
                thread::sleep(Duration::from_secs(1));

                let data = Clipboard::data();
                if data.is_empty() || data == "unknown format" {
                    continue;
                }

                if data != recent_value {
                    recent_value = data.clone();

                    let is_from_network = {
                        let mut history = clipboard_history.lock().unwrap();
                        let matched = history.contains(&data);
                        if matched {
                            history.retain(|h| h != &data);
                        }
                        matched
                    };

                    if !is_from_network {
                        log::info!("Server: Local clipboard changed; broadcasting update");
                        let payload =
                            if data.contains('\n') && (data.contains('/') || data.contains('\\')) {
                                if let Some(file_payload) = format_clipboard_data(&data) {
                                    file_payload
                                } else {
                                    ClipboardPayload::Text { text: data }
                                }
                            } else {
                                ClipboardPayload::Text { text: data }
                            };

                        let s_data = {
                            let s = settings.lock().unwrap();
                            s.clone()
                        };
                        tcp_server.broadcast_clipboard(&payload, None, &s_data);
                    }
                }
            }
        });
    }

    // --- Client Mode ---

    fn start_client(&self, settings: SettingsData) {
        log::info!("Starting Client Mode");

        let server_ip = settings.ip.clone();
        let is_connected = self.is_connected.clone();
        let udp_client = self.udp_client.clone();
        let settings_clone = settings.clone();

        let on_connect = move || {
            log::info!("Client: Connected to server");
            {
                let mut conn = is_connected.lock().unwrap();
                *conn = true;
            }

            let _ = udp_client.start(&server_ip, settings_clone.clone());
        };

        let is_connected_disc = self.is_connected.clone();
        let udp_client_disc = self.udp_client.clone();
        let on_disconnect = move || {
            log::info!("Client: Disconnected from server");
            {
                let mut conn = is_connected_disc.lock().unwrap();
                *conn = false;
            }
            udp_client_disc.stop();
        };

        let clipboard_history = self.clipboard_history.clone();
        let on_clipboard_recv = move |payload: ClipboardPayload| {
            let data_repr = match &payload {
                ClipboardPayload::Text { text } => text.clone(),
                ClipboardPayload::Files { files } => format!("files:{}", files.len()),
            };
            log::info!("Client: Received clipboard update from server: {}", data_repr);

            {
                let mut history = clipboard_history.lock().unwrap();
                history.push(data_repr);
            }

            match payload {
                ClipboardPayload::Text { text } => {
                    Clipboard::set_text(&text);
                }
                ClipboardPayload::Files { files } => {
                    let local_paths = write_clipboard_files(&files);
                    Clipboard::set_files(local_paths);
                }
            }
        };

        let tcp_client = self.tcp_client.clone();
        let ip = settings.ip.clone();

        thread::spawn(move || {
            if let Err(e) = tcp_client.connect(
                &ip,
                settings.clone(),
                on_connect,
                on_disconnect,
                on_clipboard_recv,
            ) {
                log::error!("TCP Client failed to connect: {:?}", e);
            }
        });

        self.spawn_client_clipboard_monitoring();
    }

    fn spawn_client_clipboard_monitoring(&self) {
        let is_running = self.is_running.clone();
        let clipboard_history = self.clipboard_history.clone();
        let tcp_client = self.tcp_client.clone();
        let settings = self.settings.clone();

        thread::spawn(move || {
            let mut recent_value = String::new();

            while *is_running.lock().unwrap() {
                thread::sleep(Duration::from_secs(1));

                let data = Clipboard::data();
                if data.is_empty() || data == "unknown format" {
                    continue;
                }

                if data != recent_value {
                    recent_value = data.clone();

                    let is_from_network = {
                        let mut history = clipboard_history.lock().unwrap();
                        let matched = history.contains(&data);
                        if matched {
                            history.retain(|h| h != &data);
                        }
                        matched
                    };

                    if !is_from_network {
                        log::info!("Client: Local clipboard changed; sending to server");
                        let payload =
                            if data.contains('\n') && (data.contains('/') || data.contains('\\')) {
                                if let Some(file_payload) = format_clipboard_data(&data) {
                                    file_payload
                                } else {
                                    ClipboardPayload::Text { text: data }
                                }
                            } else {
                                ClipboardPayload::Text { text: data }
                            };

                        let s_data = {
                            let s = settings.lock().unwrap();
                            s.clone()
                        };
                        let _ = tcp_client.send_clipboard(&payload, &s_data);
                    }
                }
            }
        });
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
    settings: &Arc<Mutex<SettingsData>>,
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
            let clients = active_clients.lock().unwrap();
            if let Some(c) = clients.get(&old_client) {
                if let Some(udp) = udp_server.lock().unwrap().as_ref() {
                    if let Some(addr) = c.udp_addr {
                        let s_data = settings.lock().unwrap().clone();
                        let _ = udp.send_event(&InputEvent::Stop, addr, &s_data);
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
        let clients = active_clients.lock().unwrap();
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
            if let Some(c) = clients.get(&old_client) {
                if let Some(udp) = udp_server.lock().unwrap().as_ref() {
                    if let Some(addr) = c.udp_addr {
                        let s_data = settings.lock().unwrap().clone();
                        let _ = udp.send_event(&InputEvent::Stop, addr, &s_data);
                    }
                }
            }

            drop(clients);
            {
                let mut curr = current_controlled.lock().unwrap();
                *curr = target.to_string();
            }

            {
                let mut clients = active_clients.lock().unwrap();
                if let Some(c) = clients.get_mut(target) {
                    c.mouse_x = enter_pos.0;
                    c.mouse_y = enter_pos.1;
                }
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

fn write_clipboard_files(files: &[crate::network::protocol::ClipboardFile]) -> Vec<String> {
    let temp_dir = std::env::temp_dir().join("flow");
    log::debug!("Clipboard: Writing {} received files/directories to temporary folder {:?}", files.len(), temp_dir);
    let _ = std::fs::remove_dir_all(&temp_dir);
    if let Err(e) = std::fs::create_dir_all(&temp_dir) {
        log::error!("Clipboard: Failed to create temp directory {:?}: {:?}", temp_dir, e);
    }

    let mut top_level_paths = Vec::new();

    for file in files {
        let dest_path = temp_dir.join(&file.name);
        if let Some(parent) = dest_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if file.is_dir {
            if let Err(e) = std::fs::create_dir_all(&dest_path) {
                log::error!("Clipboard: Failed to create directory {:?}: {:?}", dest_path, e);
            }
        } else if let Some(ref data) = file.data {
            if let Err(e) = std::fs::write(&dest_path, data) {
                log::error!("Clipboard: Failed to write file {:?}: {:?}", dest_path, e);
            }
        }

        let mut components = dest_path.strip_prefix(&temp_dir).unwrap().components();
        if let Some(first_comp) = components.next() {
            let top_level = temp_dir.join(first_comp.as_os_str());
            let path_str = top_level.to_string_lossy().to_string();
            if !top_level_paths.contains(&path_str) {
                top_level_paths.push(path_str);
            }
        }
    }

    top_level_paths
}
