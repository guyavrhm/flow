use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::engine::{layout, ClientInfo};
use crate::hardware::{KeyboardListener, MouseController, MouseListener};
use crate::network::protocol::{InputEvent, MonitorInfo};
use crate::network::udp::UdpServer;
use crate::state::{IS_REDIRECTING, STATE_MANAGER, FORCE_EMERGENCY_RELEASE};

pub struct ServerEdgeTracker {
    is_running: Arc<Mutex<bool>>,
    active_clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
    current_controlled: Arc<Mutex<String>>,
    mouse_listener: Arc<Mutex<Option<MouseListener>>>,
    keyboard_listener: Arc<Mutex<Option<KeyboardListener>>>,
    udp_server: Arc<Mutex<Option<UdpServer>>>,
    global_mouse_x: Arc<Mutex<i32>>,
    global_mouse_y: Arc<Mutex<i32>>,
}

impl ServerEdgeTracker {
    pub fn new(
        is_running: Arc<Mutex<bool>>,
        active_clients: Arc<Mutex<HashMap<String, ClientInfo>>>,
        current_controlled: Arc<Mutex<String>>,
        mouse_listener: Arc<Mutex<Option<MouseListener>>>,
        keyboard_listener: Arc<Mutex<Option<KeyboardListener>>>,
        udp_server: Arc<Mutex<Option<UdpServer>>>,
        global_mouse_x: Arc<Mutex<i32>>,
        global_mouse_y: Arc<Mutex<i32>>,
    ) -> Self {
        Self {
            is_running,
            active_clients,
            current_controlled,
            mouse_listener,
            keyboard_listener,
            udp_server,
            global_mouse_x,
            global_mouse_y,
        }
    }

    pub fn spawn(&self) {
        let is_running = self.is_running.clone();
        let active_clients = self.active_clients.clone();
        let current_controlled = self.current_controlled.clone();
        let mouse_listener = self.mouse_listener.clone();
        let keyboard_listener = self.keyboard_listener.clone();
        let udp_server = self.udp_server.clone();
        let global_mouse_x = self.global_mouse_x.clone();
        let global_mouse_y = self.global_mouse_y.clone();

        // 1. Initialize persistent Input Dispatch Channel
        enum DispatchEvent {
            Move { dx: i32, dy: i32 },
            Click { btn: String, pressed: bool },
            Scroll { dx: i32, dy: i32 },
            Key { key: String, pressed: bool },
        }

        let (dispatch_tx, dispatch_rx) = std::sync::mpsc::channel::<DispatchEvent>();
        let last_return_instant = Arc::new(Mutex::new(std::time::Instant::now() - Duration::from_secs(10)));
        let last_return_dispatch = last_return_instant.clone();
        let last_return_tracker = last_return_instant.clone();

        // Ultra-lightweight non-blocking OS hook callbacks (no mutex contention, no network I/O)
        let tx_move = dispatch_tx.clone();
        let on_move = move |dx: i32, dy: i32| {
            if IS_REDIRECTING.load(Ordering::Relaxed) {
                let _ = tx_move.send(DispatchEvent::Move { dx, dy });
            }
        };

        let tx_click = dispatch_tx.clone();
        let on_click = move |_x: i32, _y: i32, btn: String, pressed: bool| {
            if IS_REDIRECTING.load(Ordering::Relaxed) {
                let _ = tx_click.send(DispatchEvent::Click { btn, pressed });
            }
        };

        let tx_scroll = dispatch_tx.clone();
        let on_scroll = move |_x: i32, _y: i32, dx: i32, dy: i32| {
            if IS_REDIRECTING.load(Ordering::Relaxed) {
                let _ = tx_scroll.send(DispatchEvent::Scroll { dx, dy });
            }
        };

        let tx_press = dispatch_tx.clone();
        let on_press = move |key: String| {
            if IS_REDIRECTING.load(Ordering::Relaxed) {
                let _ = tx_press.send(DispatchEvent::Key { key, pressed: true });
            }
        };

        let tx_release = dispatch_tx;
        let on_release = move |key: String| {
            if IS_REDIRECTING.load(Ordering::Relaxed) {
                let _ = tx_release.send(DispatchEvent::Key { key, pressed: false });
            }
        };

        // High-frequency in-memory cache of active layouts for tracker and dispatch worker
        let initial_layouts = crate::config::get_all_monitor_layouts().unwrap_or_default();
        let active_layouts = Arc::new(std::sync::RwLock::new(initial_layouts));
        let active_layouts_dispatch = active_layouts.clone();
        let active_layouts_tracker = active_layouts.clone();

        // 2. Spawn dedicated input event processing and network dispatch thread
        let is_running_dispatch = is_running.clone();
        let active_clients_dispatch = active_clients.clone();
        let current_controlled_dispatch = current_controlled.clone();
        let udp_server_dispatch = udp_server.clone();
        let global_mouse_x_dispatch = global_mouse_x.clone();
        let global_mouse_y_dispatch = global_mouse_y.clone();

        thread::spawn(move || {
            while *is_running_dispatch.lock().unwrap() {
                let event = match dispatch_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(ev) => ev,
                    Err(_) => continue,
                };

                if !IS_REDIRECTING.load(Ordering::Relaxed) {
                    continue;
                }

                let curr = current_controlled_dispatch.lock().unwrap().clone();
                if curr == "main" {
                    continue;
                }

                match event {
                    DispatchEvent::Move { dx, dy } => {
                        let all_monitors = active_layouts_dispatch.read().unwrap().clone();
                        let (gx, gy) = {
                            let mut gmx = global_mouse_x_dispatch.lock().unwrap();
                            let mut gmy = global_mouse_y_dispatch.lock().unwrap();
                            *gmx += dx;
                            *gmy += dy;
                            (*gmx, *gmy)
                        };

                        // Find which monitor contains the new coordinates
                        let mut current_mon = None;
                        for m in &all_monitors {
                            if gx >= m.x && gx < m.x + m.width && gy >= m.y && gy < m.y + m.height {
                                current_mon = Some(m.clone());
                                break;
                            }
                        }

                        if let Some(mon) = current_mon {
                            if mon.host == "main" {
                                log::info!("[KVM] Mouse crossed border back into Host (main)");
                                {
                                    let mut clients = active_clients_dispatch.lock().unwrap();
                                    if let Some(c) = clients.get_mut(&curr) {
                                        if let Some(udp) = udp_server_dispatch.lock().unwrap().as_ref() {
                                            if let Some(addr) = c.udp_addr {
                                                c.udp_seq += 1;
                                                let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, c.udp_seq);
                                            }
                                        }
                                    }
                                }
                                {
                                    *current_controlled_dispatch.lock().unwrap() = "main".to_string();
                                }

                                STATE_MANAGER.transition_to_local();

                                // Warp server mouse slightly inside host monitor to avoid immediate edge re-triggering
                                let mut lx = mon.local_x + (gx - mon.x);
                                let mut ly = mon.local_y + (gy - mon.y);

                                // Clamp with inward padding of 12px away from boundaries
                                if lx <= mon.local_x + 5 {
                                    lx = mon.local_x + 12;
                                } else if lx >= mon.local_x + mon.width - 5 {
                                    lx = mon.local_x + mon.width - 12;
                                }
                                if ly <= mon.local_y + 5 {
                                    ly = mon.local_y + 12;
                                } else if ly >= mon.local_y + mon.height - 5 {
                                    ly = mon.local_y + mon.height - 12;
                                }

                                MouseController::new().set_position((lx, ly));
                                *last_return_dispatch.lock().unwrap() = std::time::Instant::now();
                            } else if mon.host != curr {
                                log::info!("[KVM] Transitioning directly between clients: {} -> {}", curr, mon.host);
                                if STATE_MANAGER.request_transition(&mon.host, (gx, gy)) {
                                    {
                                        let mut clients = active_clients_dispatch.lock().unwrap();
                                        if let Some(c) = clients.get_mut(&curr) {
                                            if let Some(udp) = udp_server_dispatch.lock().unwrap().as_ref() {
                                                if let Some(addr) = c.udp_addr {
                                                    c.udp_seq += 1;
                                                    let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, c.udp_seq);
                                                }
                                            }
                                        }
                                    }
                                    {
                                        *current_controlled_dispatch.lock().unwrap() = mon.host.clone();
                                    }
                                    send_warp_to_client(&mon.host, gx, gy, Some(&mon), &active_clients_dispatch, &udp_server_dispatch);
                                }
                            } else {
                                send_warp_to_client(&curr, gx, gy, Some(&mon), &active_clients_dispatch, &udp_server_dispatch);
                            }
                        } else {
                            let prev_mon = layout::find_closest_client_monitor(gx, gy, &curr, &all_monitors);
                            if let Some(pm) = prev_mon {
                                let (clamped_x, clamped_y) = layout::clamp_to_monitor(gx, gy, &pm);
                                {
                                    *global_mouse_x_dispatch.lock().unwrap() = clamped_x;
                                    *global_mouse_y_dispatch.lock().unwrap() = clamped_y;
                                }
                                send_warp_to_client(&curr, clamped_x, clamped_y, Some(&pm), &active_clients_dispatch, &udp_server_dispatch);
                            }
                        }
                    }
                    DispatchEvent::Click { btn, pressed } => {
                        let mut clients = active_clients_dispatch.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_dispatch.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::MouseClick { button: btn, pressed },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    }
                    DispatchEvent::Scroll { dx, dy } => {
                        let mut clients = active_clients_dispatch.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_dispatch.lock().unwrap().as_ref() {
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
                    }
                    DispatchEvent::Key { key, pressed } => {
                        let mut clients = active_clients_dispatch.lock().unwrap();
                        if let Some(c) = clients.get_mut(&curr) {
                            if let Some(udp) = udp_server_dispatch.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let seq = c.udp_seq;
                                    let _ = udp.send_event(
                                        &InputEvent::KeyPress { key, pressed },
                                        addr,
                                        &c.cryptor,
                                        seq,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });

        // 3. Register native OS hooks
        let ml = MouseListener::new(on_move, on_click, on_scroll, true);
        ml.start();
        {
            let mut ml_lock = mouse_listener.lock().unwrap();
            *ml_lock = Some(ml);
        }

        let kl = KeyboardListener::new(on_press, on_release, true);
        kl.start();
        {
            let mut kl_lock = keyboard_listener.lock().unwrap();
            *kl_lock = Some(kl);
        }

        // 4. Start Background Edge Tracking Loop
        thread::spawn(move || {
            let mouse_ctrl = MouseController::new();
            let mut last_layout_refresh = std::time::Instant::now();

            while *is_running.lock().unwrap() {
                thread::sleep(Duration::from_millis(10));

                // Periodically refresh active layouts cache in memory for the event tap thread
                if last_layout_refresh.elapsed() >= Duration::from_millis(150) {
                    last_layout_refresh = std::time::Instant::now();
                    let connected_peers = STATE_MANAGER.get_fully_connected_peers();
                    if let Ok(db_mons) = crate::config::get_all_monitor_layouts() {
                        let filtered: Vec<_> = db_mons
                            .into_iter()
                            .filter(|m| m.host == "main" || connected_peers.contains(&m.host))
                            .collect();
                        *active_layouts_tracker.write().unwrap() = filtered;
                    }
                }

                // Check emergency release flag
                if FORCE_EMERGENCY_RELEASE.swap(false, Ordering::SeqCst) {
                    log::warn!("[KVM] Edge tracker detected Emergency Release: restoring control to Host");
                    let mut curr = current_controlled.lock().unwrap();
                    if *curr != "main" {
                        let mut clients = active_clients.lock().unwrap();
                        if let Some(c) = clients.get_mut(&*curr) {
                            if let Some(udp) = udp_server.lock().unwrap().as_ref() {
                                if let Some(addr) = c.udp_addr {
                                    c.udp_seq += 1;
                                    let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, c.udp_seq);
                                    log::info!("[KVM] Sent InputEvent::Stop to [{}] on emergency release", *curr);
                                }
                            }
                        }
                    }
                    *curr = "main".to_string();
                }

                let is_main = {
                    let curr = current_controlled.lock().unwrap();
                    *curr == "main"
                };

                if is_main {
                    // Track local mouse coordinates
                    let pos = mouse_ctrl.position(); // local coordinates (lx, ly)

                    // Retrieve local monitors (server monitors)
                    let local_mons = crate::hardware::get_monitors();
                    let mut found_mon = None;
                    for m in &local_mons {
                        if pos.0 >= m.local_x
                            && pos.0 < m.local_x + m.width
                            && pos.1 >= m.local_y
                            && pos.1 < m.local_y + m.height
                        {
                            found_mon = Some(m.clone());
                            break;
                        }
                    }

                    let active_mon = found_mon.unwrap_or_else(|| {
                        local_mons.first().cloned().unwrap_or(MonitorInfo {
                            name: "Main Display".to_string(),
                            local_x: 0,
                            local_y: 0,
                            width: 1920,
                            height: 1080,
                            scale_factor: 1.0,
                        })
                    });

                    // Query the DB coordinate layout for this server monitor
                    let db_mon = crate::config::get_all_monitor_layouts()
                        .ok()
                        .and_then(|lays| {
                            lays.into_iter().find(|l| l.host == "main" && l.monitor_name == active_mon.name)
                        })
                        .unwrap_or_else(|| crate::config::MonitorLayout {
                            monitor_id: format!("main_{}", active_mon.name),
                            host: "main".to_string(),
                            monitor_name: active_mon.name.clone(),
                            x: active_mon.local_x,
                            y: active_mon.local_y,
                            width: active_mon.width,
                            height: active_mon.height,
                            scale_factor: active_mon.scale_factor,
                            local_x: active_mon.local_x,
                            local_y: active_mon.local_y,
                        });

                    // Compute global coordinate (gx, gy)
                    let (gx, gy) = layout::project_local_to_global(pos.0, pos.1, &db_mon, &active_mon);

                    {
                        *global_mouse_x.lock().unwrap() = gx;
                        *global_mouse_y.lock().unwrap() = gy;
                    }

                    // Check if mouse is near any edge of active_mon to transition
                    let mut target_screen: Option<(String, crate::config::MonitorLayout)> = None;
                    let mut enter_pos = (0, 0); // global position

                    // Check transition hysteresis cooldown after returning to host (prevent ping-pong border bounce)
                    if last_return_tracker.lock().unwrap().elapsed() < Duration::from_millis(300) {
                        continue;
                    }

                    // SAFETY INVARIANT: Only check transitions against FULLY CONNECTED clients
                    let connected_peers = STATE_MANAGER.get_fully_connected_peers();
                    if !connected_peers.is_empty() {
                        let valid_layouts = active_layouts_tracker.read().unwrap().clone();

                        // Checks: Left edge
                        if pos.0 < active_mon.local_x + 5 {
                            let gx_proj = db_mon.x - 8;
                            let gy_proj = gy;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &valid_layouts) {
                                target_screen = Some((target.host.clone(), target.clone()));
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Right edge
                        else if pos.0 > active_mon.local_x + active_mon.width - 5 {
                            let gx_proj = db_mon.x + db_mon.width + 8;
                            let gy_proj = gy;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &valid_layouts) {
                                target_screen = Some((target.host.clone(), target.clone()));
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Top edge
                        else if pos.1 < active_mon.local_y + 5 {
                            let gx_proj = gx;
                            let gy_proj = db_mon.y - 8;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &valid_layouts) {
                                target_screen = Some((target.host.clone(), target.clone()));
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Bottom edge
                        else if pos.1 > active_mon.local_y + active_mon.height - 5 {
                            let gx_proj = gx;
                            let gy_proj = db_mon.y + db_mon.height + 8;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &valid_layouts) {
                                target_screen = Some((target.host.clone(), target.clone()));
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                    }

                    if let Some((target_ip, target_mon)) = target_screen {
                        // Request transition through StateManager guard
                        if STATE_MANAGER.request_transition(&target_ip, enter_pos) {
                            {
                                let mut curr = current_controlled.lock().unwrap();
                                *curr = target_ip.clone();
                            }
                            {
                                *global_mouse_x.lock().unwrap() = enter_pos.0;
                                *global_mouse_y.lock().unwrap() = enter_pos.1;
                            }

                            // Send warp to client
                            send_warp_to_client(
                                &target_ip,
                                enter_pos.0,
                                enter_pos.1,
                                Some(&target_mon),
                                &active_clients,
                                &udp_server,
                            );
                        }
                    }
                }
            }
        });
    }
}

fn send_warp_to_client(
    ip: &str,
    gx: i32,
    gy: i32,
    layout_opt: Option<&crate::config::MonitorLayout>,
    active_clients: &Arc<Mutex<HashMap<String, ClientInfo>>>,
    udp_server: &Arc<Mutex<Option<UdpServer>>>,
) {
    let mut clients = active_clients.lock().unwrap();
    if let Some(c) = clients.get_mut(ip) {
        if let Some(udp) = udp_server.lock().unwrap().as_ref() {
            if let Some(addr) = c.udp_addr {
                let target_layout = match layout_opt {
                    Some(l) => Some(l.clone()),
                    None => crate::config::get_all_monitor_layouts()
                        .ok()
                        .and_then(|layouts| layouts.into_iter().find(|l| &l.host == ip)),
                };
                if let Some(layout) = target_layout {
                    let (cx, cy) = layout::project_global_to_local(
                        gx,
                        gy,
                        &layout,
                        c.uses_physical_pixels,
                    );
                    c.udp_seq += 1;
                    let seq = c.udp_seq;
                    let _ = udp.send_event(&InputEvent::Move { x: cx, y: cy }, addr, &c.cryptor, seq);
                }
            }
        }
    }
}
