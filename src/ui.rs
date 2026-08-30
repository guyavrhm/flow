pub mod canvas;
pub mod tray;

use crate::config::{
    ENCRYPTION_OFF, PC_CLIENT, PC_SERVER, SettingsData, get_settings,
    save_settings,
};
use crate::engine::AppEngine;
use crate::network::get_local_ip;
use canvas::ScreenLayoutCanvas;
use tray::SystemTrayManager;

use eframe::egui;
use std::sync::Arc;
use std::time::Duration;

pub struct FlowApp {
    settings: SettingsData,
    canvas: ScreenLayoutCanvas,
    engine: Arc<AppEngine>,
    tray: Arc<SystemTrayManager>,
    show_window: bool,
    show_trash_list: bool,
    status_msg: String,
    local_fingerprint: String,
    was_hidden_for_transfer: bool,
    last_topology_version: u64,
    last_connected_peers: Vec<String>,
}

impl FlowApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        engine: Arc<AppEngine>,
        tray: Arc<SystemTrayManager>,
    ) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(45, 120, 200);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(55, 140, 230);
        cc.egui_ctx.set_style(style);

        crate::state::STATE_MANAGER.set_egui_ctx(cc.egui_ctx.clone());

        let initial_settings = get_settings().unwrap_or(SettingsData {
            ip: "".to_string(),
            password: "".to_string(),
            pc: PC_SERVER,
            encryption: ENCRYPTION_OFF,
        });

        let mut canvas = ScreenLayoutCanvas::new();
        canvas.load_from_db(&Vec::new());

        let local_fingerprint = if let Ok((cert_pem, key_pem)) = crate::crypto::load_or_generate_cert(crate::paths::get_app_dir()) {
            let (certs, _) = crate::network::tls::load_certs_and_key(&cert_pem, &key_pem);
            if !certs.is_empty() {
                crate::crypto::compute_fingerprint(certs[0].as_ref())
            } else {
                "No certificate available".to_string()
            }
        } else {
            "Failed to load certs".to_string()
        };

        Self {
            settings: initial_settings,
            canvas,
            engine,
            tray,
            show_window: true,
            show_trash_list: false,
            status_msg: "".to_string(),
            local_fingerprint,
            was_hidden_for_transfer: false,
            last_topology_version: 0,
            last_connected_peers: Vec::new(),
        }
    }

    fn save_changes(&mut self) {
        log::info!("UI: Saving changes to settings and screen layout...");
        if let Err(e) = save_settings(&self.settings) {
            log::error!("UI: Failed to save settings to DB: {:?}", e);
            self.status_msg = format!("Failed to save settings: {:?}", e);
            return;
        }

        for screen in self.canvas.screens.values() {
            let layout = crate::config::MonitorLayout {
                monitor_id: screen.monitor_id.clone(),
                host: screen.host.clone(),
                monitor_name: screen.monitor_name.clone(),
                x: screen.x as i32,
                y: screen.y as i32,
                width: screen.w as i32,
                height: screen.h as i32,
                scale_factor: screen.scale_factor,
                local_x: screen.local_x,
                local_y: screen.local_y,
            };
            if let Err(e) = crate::config::save_monitor_layout(&layout) {
                log::error!("UI: Failed to save monitor layout to DB: {:?}", e);
            }
        }

        crate::state::STATE_MANAGER.notify_topology_changed();
        self.status_msg = "Settings saved successfully!".to_string();
        log::info!("UI: Settings and screen layout saved successfully. Triggering engine reload.");

        let engine = self.engine.clone();
        std::thread::spawn(move || {
            engine.reload();
        });
    }
}

impl eframe::App for FlowApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_window = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id.0 == self.tray.menu_settings_id {
                log::info!("Tray: Settings menu item clicked");
                self.show_window = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id.0 == self.tray.menu_help_id {
                log::info!("Tray: Help menu item clicked");
                if let Err(e) = webbrowser::open("https://guyavrhm.github.io/flow") {
                    log::error!("Tray: Failed to open help URL in browser: {:?}", e);
                }
            } else if event.id.0 == self.tray.menu_exit_id {
                log::info!("Tray: Exit menu item clicked. Terminating application.");
                self.engine.stop();
                std::process::exit(0);
            }
        }

        let is_conn = {
            let conn = self.engine.is_connected.lock().unwrap();
            *conn
        };
        if is_conn {
            self.tray.set_connected();
        } else {
            self.tray.set_disconnected();
        }

        let progress = crate::hardware::CLIPBOARD_SYNC_PROGRESS.load(std::sync::atomic::Ordering::Relaxed);
        if progress > 0 {
            if !self.show_window {
                self.show_window = true;
                self.was_hidden_for_transfer = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }

            // Draw a modal progress overlay
            egui::Window::new("Clipboard Sync")
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .collapsible(false)
                .resizable(false)
                .movable(false)
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label("Syncing large clipboard payload from network...");
                        ui.add_space(5.0);
                        if progress == 1 {
                            ui.add(egui::ProgressBar::new(0.0).show_percentage());
                        } else {
                            ui.add(egui::ProgressBar::new(progress as f32 / 100.0).show_percentage());
                        }
                    });
                });
        } else if self.was_hidden_for_transfer {
            self.show_window = false;
            self.was_hidden_for_transfer = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Auto-unhide and focus if there is a pending trust approval request
        let pending_trusts = crate::state::STATE_MANAGER.get_pending_trusts();
        if !pending_trusts.is_empty() && !self.show_window {
            log::info!("[UI] Pending trust approval detected while minimized. Unhiding window.");
            self.show_window = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        if !self.show_window {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Left Column: Configuration Settings
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    ui.heading("flow Settings");
                    ui.add_space(10.0);

                    // Mode Selection
                    ui.group(|ui| {
                        ui.label("Mode:");
                        ui.radio_value(
                            &mut self.settings.pc,
                            PC_SERVER,
                            "Server (Share Mouse/Keyboard)",
                        );
                        ui.radio_value(&mut self.settings.pc, PC_CLIENT, "Client");
                    });

                    ui.add_space(10.0);

                    // IP Config
                    ui.group(|ui| {
                        if self.settings.pc == PC_SERVER {
                            ui.label(format!("Local IP Address: {}", get_local_ip()));
                        } else {
                            ui.label("Enter Server's IP:");
                            ui.text_edit_singleline(&mut self.settings.ip);
                        }
                    });

                    ui.add_space(10.0);

                    // Local certificate fingerprint
                    ui.group(|ui| {
                        ui.label("Local Certificate Fingerprint:");
                        let mut fp = self.local_fingerprint.clone();
                        ui.add(
                            egui::TextEdit::singleline(&mut fp)
                                .interactive(false)
                        );
                        if ui.button("Copy Fingerprint").clicked() {
                            ui.output_mut(|o| o.copied_text = self.local_fingerprint.clone());
                        }
                    });

                    ui.add_space(20.0);

                    // Actions
                    ui.horizontal(|ui| {
                        if ui.button("Save & Apply").clicked() {
                            self.save_changes();
                        }
                        if ui.button("Reload").clicked() {
                            if let Ok(s) = get_settings() {
                                self.settings = s;
                            }
                            let active_ips = {
                                let clients = self.engine.active_clients.lock().unwrap();
                                clients.keys().cloned().collect::<Vec<String>>()
                            };
                            self.canvas.load_from_db(&active_ips);
                            self.status_msg = "Settings reloaded".to_string();
                        }
                        if ui.button("Hide Window").clicked() {
                            self.show_window = false;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        }
                    });

                    if !self.status_msg.is_empty() {
                        ui.add_space(10.0);
                        ui.colored_label(egui::Color32::LIGHT_GREEN, &self.status_msg);
                    }
                });

                // Separator
                ui.separator();

                // Right Column: Visual Layout Canvas
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Screen Topology Map");
                        ui.add_space(10.0);

                        let trash_btn_label = if self.show_trash_list {
                            "Hide Trash"
                        } else {
                            "Show Trash"
                        };
                        if ui.button(trash_btn_label).clicked() {
                            self.show_trash_list = !self.show_trash_list;
                        }
                    });

                    ui.add_space(5.0);

                    ui.horizontal(|ui| {
                        let fully_connected_ips = crate::state::STATE_MANAGER.get_fully_connected_peers();
                        let current_version = crate::state::STATE_MANAGER.topology_version();
                        if current_version != self.last_topology_version || fully_connected_ips != self.last_connected_peers {
                            self.canvas.sync_with_db(&fully_connected_ips);
                            self.last_topology_version = current_version;
                            self.last_connected_peers = fully_connected_ips;
                        }

                        let _ = self.canvas.draw(ui);

                        if self.show_trash_list {
                            ui.vertical(|ui| {
                                ui.label("Trash Bin (Double-Click to Delete Host):");
                                egui::ScrollArea::vertical()
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        let mut hosts = self.canvas.screens.values()
                                            .map(|s| s.host.clone())
                                            .filter(|h| h != "main")
                                            .collect::<Vec<String>>();
                                        hosts.sort();
                                        hosts.dedup();

                                        let mut to_remove = None;
                                        for host in hosts {
                                            let is_sel = self.canvas.selected_screen.as_ref()
                                                .map_or(false, |id| self.canvas.screens.get(id).map_or(false, |s| s.host == host));
                                            if ui
                                                .selectable_label(is_sel, &host)
                                                .double_clicked()
                                            {
                                                to_remove = Some(host);
                                            }
                                        }
                                        if let Some(r_host) = to_remove {
                                            log::info!("UI: Deleting host layouts: {}", r_host);
                                            if let Err(e) = crate::config::remove_monitor_layouts_by_host(&r_host) {
                                                log::error!("UI: Failed to remove host {} from DB: {:?}", r_host, e);
                                            }
                                            self.canvas.screens.retain(|_, s| s.host != r_host);
                                            self.canvas.selected_screen = None;
                                            crate::state::STATE_MANAGER.notify_topology_changed();
                                        }
                                    });
                            });
                        }
                    });
                });
            });
        });

        // Show TOFU fingerprint verification modal if any connection is pending trust approval
        let pending_request = {
            let trusts = crate::state::STATE_MANAGER.get_pending_trusts();
            trusts.first().cloned()
        };

        if let Some(req) = pending_request {
            let title = if req.is_mismatch {
                "⚠️ Security Alert - Fingerprint Mismatch!"
            } else {
                "Security Alert - Untrusted Connection"
            };

            egui::Window::new(title)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if req.is_mismatch {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 80, 80),
                            "⚠️ CRITICAL SECURITY WARNING: Certificate fingerprint changed for this known host!",
                        );
                        ui.label("This may happen if the other computer re-installed the app, or someone is intercepting the connection.");
                    } else {
                        ui.colored_label(egui::Color32::from_rgb(255, 215, 0), "⚠️ A new computer is requesting a connection.");
                    }

                    ui.add_space(5.0);
                    ui.label(format!("IP Address: {}", req.ip));
                    ui.add_space(5.0);
                    ui.label("SHA-256 Certificate Fingerprint:");
                    ui.code(&req.fingerprint);
                    ui.add_space(10.0);
                    ui.label("Please compare this fingerprint with the code shown on the other computer's screen. If they match, it is safe to connect.");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Trust and Connect").clicked() {
                            log::info!("[UI] User clicked 'Trust and Connect' for [{}]", req.ip);
                            crate::state::STATE_MANAGER.approve_trust(&req.ip);
                            let mut list = self.engine.pending_trusts.lock().unwrap();
                            list.retain(|r| r.ip != req.ip);
                        }
                        if ui.button("Reject Connection").clicked() {
                            log::warn!("[UI] User clicked 'Reject Connection' for [{}]", req.ip);
                            crate::state::STATE_MANAGER.reject_trust(&req.ip);
                            let mut list = self.engine.pending_trusts.lock().unwrap();
                            list.retain(|r| r.ip != req.ip);
                        }
                    });
                });
        }
    }
}
