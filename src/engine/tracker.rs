use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crate::hardware::{MouseController, MouseListener, KeyboardListener};
use crate::network::protocol::{InputEvent, MonitorInfo};
use crate::network::udp::UdpServer;
use crate::engine::{ClientInfo, layout};
use std::collections::HashMap;

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

        thread::spawn(move || {
            let mouse_ctrl = MouseController::new();

            while *is_running.lock().unwrap() {
                thread::sleep(Duration::from_millis(10));

                let is_main = {
                    let curr = current_controlled.lock().unwrap();
                    *curr == "main"
                };

                if is_main {
                    // Track local mouse coordinates
                    let pos = mouse_ctrl.position(); // local coordinates (lx, ly)
                    
                    // Retrieve local monitors (server monitors)
                    let local_mons = crate::hardware::get_monitors();
                    // Let's find which monitor containing the mouse
                    let mut found_mon = None;
                    for m in &local_mons {
                        if pos.0 >= m.local_x && pos.0 < m.local_x + m.width
                            && pos.1 >= m.local_y && pos.1 < m.local_y + m.height {
                            found_mon = Some(m.clone());
                            break;
                        }
                    }
                    // Fallback to first if none contains the coordinate
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
                    let db_mon = crate::config::get_all_monitor_layouts().ok().and_then(|lays| {
                        lays.into_iter().find(|l| l.host == "main" && l.monitor_name == active_mon.name)
                    }).unwrap_or_else(|| crate::config::MonitorLayout {
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
                    let mut target_screen: Option<String> = None;
                    let mut enter_pos = (0, 0); // global position

                    if let Ok(layouts) = crate::config::get_all_monitor_layouts() {
                        // Checks: Left edge
                        if pos.0 < active_mon.local_x + 5 {
                            let gx_proj = db_mon.x - 8;
                            let gy_proj = gy;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &layouts) {
                                target_screen = Some(target.host.clone());
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Right edge
                        else if pos.0 > active_mon.local_x + active_mon.width - 5 {
                            let gx_proj = db_mon.x + db_mon.width + 8;
                            let gy_proj = gy;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &layouts) {
                                target_screen = Some(target.host.clone());
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Top edge
                        else if pos.1 < active_mon.local_y + 5 {
                            let gx_proj = gx;
                            let gy_proj = db_mon.y - 8;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &layouts) {
                                target_screen = Some(target.host.clone());
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                        // Bottom edge
                        else if pos.1 > active_mon.local_y + active_mon.height - 5 {
                            let gx_proj = gx;
                            let gy_proj = db_mon.y + db_mon.height + 8;
                            if let Some(target) = layout::find_client_monitor_containing(gx_proj, gy_proj, &layouts) {
                                target_screen = Some(target.host.clone());
                                enter_pos = (gx_proj, gy_proj);
                            }
                        }
                    }

                    if let Some(target_ip) = target_screen {
                        log::info!("Transitioning control to client: {}", target_ip);
                        {
                            let mut curr = current_controlled.lock().unwrap();
                            *curr = target_ip.clone();
                        }
                        {
                            *global_mouse_x.lock().unwrap() = enter_pos.0;
                            *global_mouse_y.lock().unwrap() = enter_pos.1;
                        }

                        // Send warp to client
                        send_warp_to_client(&target_ip, enter_pos.0, enter_pos.1, &active_clients, &udp_server);

                        // Start input listeners on server
                        let active_clients_cb = active_clients.clone();
                        let current_controlled_cb = current_controlled.clone();
                        let udp_server_cb = udp_server.clone();
                        let global_mouse_x_cb = global_mouse_x.clone();
                        let global_mouse_y_cb = global_mouse_y.clone();

                        let on_move = move |dx: i32, dy: i32| {
                            let curr = current_controlled_cb.lock().unwrap().clone();
                            if curr == "main" {
                                return;
                            }

                            // Load active monitors (server + active clients)
                            let mut all_monitors = Vec::new();
                            if let Ok(db_mons) = crate::config::get_all_monitor_layouts() {
                                let clients = active_clients_cb.lock().unwrap();
                                for m in db_mons {
                                    if m.host == "main" || clients.contains_key(&m.host) {
                                        all_monitors.push(m);
                                    }
                                }
                            }

                            let (gx, gy) = {
                                let mut gmx = global_mouse_x_cb.lock().unwrap();
                                let mut gmy = global_mouse_y_cb.lock().unwrap();
                                *gmx += dx;
                                *gmy += dy;
                                (*gmx, *gmy)
                            };

                            // Find which monitor containing the new coordinates
                            let mut current_mon = None;
                            for m in &all_monitors {
                                if gx >= m.x && gx < m.x + m.width
                                    && gy >= m.y && gy < m.y + m.height {
                                    current_mon = Some(m.clone());
                                    break;
                                }
                            }

                            if let Some(mon) = current_mon {
                                if mon.host == "main" {
                                    // Transition back to server
                                    log::info!("Transitioning control back to server");
                                    {
                                        let mut clients = active_clients_cb.lock().unwrap();
                                        if let Some(c) = clients.get_mut(&curr) {
                                            if let Some(udp) = udp_server_cb.lock().unwrap().as_ref() {
                                                if let Some(addr) = c.udp_addr {
                                                    c.udp_seq += 1;
                                                    let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, c.udp_seq);
                                                }
                                            }
                                        }
                                    }
                                    {
                                        *current_controlled_cb.lock().unwrap() = "main".to_string();
                                    }
                                    // Warp server mouse to mon.local_x + offset
                                    let lx = mon.local_x + (gx - mon.x);
                                    let ly = mon.local_y + (gy - mon.y);
                                    MouseController::new().set_position((lx, ly));
                                } else if mon.host != curr {
                                    // Transition between different clients
                                    log::info!("Transitioning directly between clients: {} -> {}", curr, mon.host);
                                    {
                                        let mut clients = active_clients_cb.lock().unwrap();
                                        if let Some(c) = clients.get_mut(&curr) {
                                            if let Some(udp) = udp_server_cb.lock().unwrap().as_ref() {
                                                if let Some(addr) = c.udp_addr {
                                                    c.udp_seq += 1;
                                                    let _ = udp.send_event(&InputEvent::Stop, addr, &c.cryptor, c.udp_seq);
                                                }
                                            }
                                        }
                                    }
                                    {
                                        *current_controlled_cb.lock().unwrap() = mon.host.clone();
                                    }
                                    send_warp_to_client(&mon.host, gx, gy, &active_clients_cb, &udp_server_cb);
                                } else {
                                    // Move within the same client
                                    send_warp_to_client(&curr, gx, gy, &active_clients_cb, &udp_server_cb);
                                }
                            } else {
                                // Clamp to previous monitor
                                let prev_mon = layout::find_closest_client_monitor(gx, gy, &curr, &all_monitors);
                                if let Some(pm) = prev_mon {
                                    let (clamped_x, clamped_y) = layout::clamp_to_monitor(gx, gy, &pm);
                                    {
                                        *global_mouse_x_cb.lock().unwrap() = clamped_x;
                                        *global_mouse_y_cb.lock().unwrap() = clamped_y;
                                    }
                                    send_warp_to_client(&curr, clamped_x, clamped_y, &active_clients_cb, &udp_server_cb);
                                }
                            }
                        };

                        let udp_server_click = udp_server.clone();
                        let active_clients_click = active_clients.clone();
                        let current_controlled_click = current_controlled.clone();

                        let on_click = move |_x: i32, _y: i32, btn: String, pressed: bool| {
                            let curr = current_controlled_click.lock().unwrap().clone();
                            if curr == "main" {
                                return;
                            }
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
                            if curr == "main" {
                                return;
                            }
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
                        {
                            let mut ml_lock = mouse_listener.lock().unwrap();
                            *ml_lock = Some(ml);
                        }

                        let udp_server_press = udp_server.clone();
                        let active_clients_press = active_clients.clone();
                        let current_controlled_press = current_controlled.clone();

                        let on_press = move |key: String| {
                            let curr = current_controlled_press.lock().unwrap().clone();
                            if curr == "main" {
                                return;
                            }
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
                            if curr == "main" {
                                return;
                            }
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
                        {
                            let mut kl_lock = keyboard_listener.lock().unwrap();
                            *kl_lock = Some(kl);
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
    active_clients: &Arc<Mutex<HashMap<String, ClientInfo>>>,
    udp_server: &Arc<Mutex<Option<UdpServer>>>,
) {
    let mut clients = active_clients.lock().unwrap();
    if let Some(c) = clients.get_mut(ip) {
        if let Some(udp) = udp_server.lock().unwrap().as_ref() {
            if let Some(addr) = c.udp_addr {
                // Find screen layout in DB to map global coords back to client's local coords
                if let Ok(layouts) = crate::config::get_all_monitor_layouts() {
                    if let Some(layout) = layouts.into_iter().find(|l| &l.host == ip) {
                        let (cx, cy) = layout::project_global_to_local(gx, gy, &layout, c.uses_physical_pixels);
                        c.udp_seq += 1;
                        let seq = c.udp_seq;
                        let _ = udp.send_event(&InputEvent::Move { x: cx, y: cy }, addr, &c.cryptor, seq);
                    }
                }
            }
        }
    }
}
