pub mod canvas;
pub mod tray;

use crate::config::{
    ENCRYPTION_OFF, ENCRYPTION_ON, PC_CLIENT, PC_SERVER, SettingsData, get_settings, remove_screen,
    save_settings, update_screen,
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

        let initial_settings = get_settings().unwrap_or(SettingsData {
            ip: "".to_string(),
            password: "".to_string(),
            pc: PC_SERVER,
            encryption: ENCRYPTION_OFF,
        });

        let mut canvas = ScreenLayoutCanvas::new();
        canvas.load_from_db(&Vec::new());

        Self {
            settings: initial_settings,
            canvas,
            engine,
            tray,
            show_window: true,
            show_trash_list: false,
            status_msg: "".to_string(),
        }
    }

    fn save_changes(&mut self) {
        log::info!("UI: Saving changes to settings and screen layout...");
        if let Err(e) = save_settings(&self.settings) {
            log::error!("UI: Failed to save settings to DB: {:?}", e);
            self.status_msg = format!("Failed to save settings: {:?}", e);
            return;
        }

        let computed = self.canvas.compute_attachments();
        for (name, att) in computed {
            if let Err(e) = update_screen(&name, &att) {
                log::error!("UI: Failed to update screen layout for {} in DB: {:?}", name, e);
            } else {
                log::debug!("UI: Updated screen attachments for {}", name);
            }
        }

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
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id.0 == self.tray.menu_settings_id {
                log::info!("Tray: Settings menu item clicked");
                self.show_window = true;
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

                    // Encryption settings
                    ui.group(|ui| {
                        let mut encrypt_bool = self.settings.encryption == ENCRYPTION_ON;
                        if ui
                            .checkbox(&mut encrypt_bool, "Enable Encryption")
                            .changed()
                        {
                            self.settings.encryption = if encrypt_bool {
                                ENCRYPTION_ON
                            } else {
                                ENCRYPTION_OFF
                            };
                        }

                        if self.settings.encryption == ENCRYPTION_ON {
                            ui.label("Password:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.settings.password)
                                    .password(true),
                            );
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
                            self.canvas.load_from_db(&Vec::new());
                            self.status_msg = "Settings reloaded".to_string();
                        }
                        if ui.button("Hide Window").clicked() {
                            self.show_window = false;
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
                        let selected = self.canvas.draw(ui);

                        if self.show_trash_list {
                            ui.vertical(|ui| {
                                ui.label("Trash Bin (Double-Click to Delete):");
                                egui::ScrollArea::vertical()
                                    .max_height(200.0)
                                    .show(ui, |ui| {
                                        let mut to_remove = None;
                                        for name in self.canvas.screens.keys() {
                                            if name != "main" {
                                                if ui
                                                    .selectable_label(
                                                        self.canvas.selected_screen.as_ref()
                                                            == Some(name),
                                                        name,
                                                    )
                                                    .double_clicked()
                                                {
                                                    to_remove = Some(name.clone());
                                                }
                                            }
                                        }
                                        if let Some(r_name) = to_remove {
                                            log::info!("UI: Deleting screen: {}", r_name);
                                            if let Err(e) = remove_screen(&r_name) {
                                                log::error!("UI: Failed to remove screen {} from DB: {:?}", r_name, e);
                                            }
                                            self.canvas.screens.remove(&r_name);
                                            if self.canvas.selected_screen.as_ref() == Some(&r_name)
                                            {
                                                self.canvas.selected_screen = None;
                                            }
                                        }
                                    });
                            });
                        }
                    });
                });
            });
        });
    }
}
