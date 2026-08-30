use crate::config::get_all_monitor_layouts;
use eframe::egui;
use std::collections::HashMap;

pub const RENDER_SCALE: f32 = 0.08; // Scaling factor for rendering logical pixels on egui canvas

#[derive(Clone, Debug)]
pub struct EditorMonitor {
    pub monitor_id: String,
    pub host: String,
    pub monitor_name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub scale_factor: f64,
    pub local_x: i32,
    pub local_y: i32,
    pub is_connected: bool,
}

impl EditorMonitor {
    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn distance(&self, other: &EditorMonitor) -> f32 {
        ((self.center_x() - other.center_x()).powi(2)
            + (self.center_y() - other.center_y()).powi(2))
        .sqrt()
    }
}

pub struct ScreenLayoutCanvas {
    pub screens: HashMap<String, EditorMonitor>,
    pub selected_screen: Option<String>,
}

impl ScreenLayoutCanvas {
    pub fn new() -> Self {
        Self {
            screens: HashMap::new(),
            selected_screen: None,
        }
    }

    pub fn load_from_db(&mut self, active_clients: &Vec<String>) {
        self.screens.clear();
        self.sync_with_db(active_clients);
    }

    pub fn sync_with_db(&mut self, active_clients: &[String]) {
        if let Ok(layouts) = get_all_monitor_layouts() {
            let mut db_ids = std::collections::HashSet::new();

            for lay in layouts {
                // Filter out loopback 127.0.0.1 monitors to avoid self-host duplicates
                if lay.host == "127.0.0.1" {
                    continue;
                }

                db_ids.insert(lay.monitor_id.clone());
                let is_connected = lay.host == "main" || active_clients.contains(&lay.host);

                if let Some(existing) = self.screens.get_mut(&lay.monitor_id) {
                    if existing.is_connected != is_connected {
                        log::info!(
                            "[UI] Canvas: Screen [{}] ({}) state changed: {} -> {}",
                            lay.host,
                            lay.monitor_name,
                            if existing.is_connected { "CONNECTED (Green)" } else { "OFFLINE (Grey)" },
                            if is_connected { "CONNECTED (Green)" } else { "OFFLINE (Grey)" }
                        );
                    }
                    existing.is_connected = is_connected;
                    // Update geometry and dimensions
                    existing.w = lay.width as f32;
                    existing.h = lay.height as f32;
                    existing.scale_factor = lay.scale_factor;
                    existing.local_x = lay.local_x;
                    existing.local_y = lay.local_y;
                } else {
                    log::info!(
                        "[UI] Canvas: Added new screen [{}] ({}) ({}x{}) - Status: {}",
                        lay.host,
                        lay.monitor_name,
                        lay.width,
                        lay.height,
                        if is_connected { "CONNECTED (Green)" } else { "OFFLINE (Grey)" }
                    );
                    self.screens.insert(
                        lay.monitor_id.clone(),
                        EditorMonitor {
                            monitor_id: lay.monitor_id,
                            host: lay.host,
                            monitor_name: lay.monitor_name,
                            x: lay.x as f32,
                            y: lay.y as f32,
                            w: lay.width as f32,
                            h: lay.height as f32,
                            scale_factor: lay.scale_factor,
                            local_x: lay.local_x,
                            local_y: lay.local_y,
                            is_connected,
                        },
                    );
                }
            }

            // Prune deleted screens
            self.screens.retain(|id, _| db_ids.contains(id));
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(500.0, 400.0), egui::Sense::click_and_drag());

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);

        // Find the main monitor at (0,0) (or any server monitor) to center the grid
        let main_screen = self
            .screens
            .values()
            .find(|s| s.host == "main" && s.x == 0.0 && s.y == 0.0)
            .or_else(|| self.screens.values().find(|s| s.host == "main"))
            .cloned();

        let (offset_x, offset_y) = if let Some(ref ms) = main_screen {
            (
                250.0 - (ms.w * RENDER_SCALE) / 2.0,
                200.0 - (ms.h * RENDER_SCALE) / 2.0,
            )
        } else {
            (250.0 - 60.0, 200.0 - 40.0)
        };

        // Draw cross at local origin center point
        let origin_pt = rect.min + egui::vec2(offset_x, offset_y);
        painter.line_segment(
            [origin_pt - egui::vec2(15.0, 0.0), origin_pt + egui::vec2(15.0, 0.0)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(100)),
        );
        painter.line_segment(
            [origin_pt - egui::vec2(0.0, 15.0), origin_pt + egui::vec2(0.0, 15.0)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(100)),
        );

        let mut dragged_screen = None;
        let mut drag_delta = egui::Vec2::ZERO;

        if response.dragged() {
            drag_delta = response.drag_delta();
        }

        for (id, screen) in self.screens.iter_mut() {
            let screen_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(offset_x + screen.x * RENDER_SCALE, offset_y + screen.y * RENDER_SCALE),
                egui::vec2(screen.w * RENDER_SCALE, screen.h * RENDER_SCALE),
            );

            if response.dragged()
                && ui
                    .input(|i| i.pointer.hover_pos())
                    .map_or(false, |pos| screen_rect.contains(pos))
            {
                if screen.host != "main" {
                    dragged_screen = Some(id.clone());
                }
            }

            let is_selected = self.selected_screen.as_ref() == Some(id);

            let fill_color = if screen.host == "main" {
                egui::Color32::from_rgb(100, 149, 237) // Cornflower Blue for Host
            } else if screen.is_connected {
                egui::Color32::from_rgb(140, 220, 140) // Connected client
            } else {
                egui::Color32::from_rgb(190, 190, 190) // Offline client
            };

            let stroke_color = if is_selected {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().widgets.active.bg_stroke.color
            };

            let stroke_width = if is_selected { 3.0_f32 } else { 1.5_f32 };

            painter.rect(
                screen_rect,
                4.0,
                fill_color,
                egui::Stroke::new(stroke_width, stroke_color),
            );

            // Text info on screen block
            let text = format!("{}\n{}", screen.host, screen.monitor_name);
            let text_pos = screen_rect.min + egui::vec2(5.0, 5.0);
            painter.text(
                text_pos,
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::proportional(11.0),
                egui::Color32::BLACK,
            );
        }

        if let Some(id) = dragged_screen {
            self.selected_screen = Some(id.clone());
            if let Some(screen) = self.screens.get_mut(&id) {
                screen.x += drag_delta.x / RENDER_SCALE;
                screen.y += drag_delta.y / RENDER_SCALE;
            }
        }

        if response.drag_stopped() {
            if let Some(id) = self.selected_screen.clone() {
                if let Some(screen) = self.screens.get(&id) {
                    if screen.host != "main" {
                        self.snap_screen(&id);
                    }
                }
            }
        }

        if response.clicked() {
            let mut clicked = None;
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                for (id, screen) in self.screens.iter() {
                    let screen_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(offset_x + screen.x * RENDER_SCALE, offset_y + screen.y * RENDER_SCALE),
                        egui::vec2(screen.w * RENDER_SCALE, screen.h * RENDER_SCALE),
                    );
                    if screen_rect.contains(pos) {
                        clicked = Some(id.clone());
                        break;
                    }
                }
            }
            self.selected_screen = clicked;
        }

        self.selected_screen.clone()
    }

    fn snap_screen(&mut self, id: &str) {
        let mut target = None;
        let mut min_dist = f32::MAX;

        let screen_val = match self.screens.get(id) {
            Some(s) => s.clone(),
            None => return,
        };

        for (other_id, other) in self.screens.iter() {
            if other_id != id {
                let d = screen_val.distance(other);
                if d < min_dist {
                    min_dist = d;
                    target = Some(other.clone());
                }
            }
        }

        if let Some(closest) = target {
            let threshold = 40.0; // 40 logical pixels snapping boundary

            let mut final_x = screen_val.x;
            let mut final_y = screen_val.y;

            // Snap A's left edge to B's right edge
            if (screen_val.x - (closest.x + closest.w)).abs() < threshold {
                final_x = closest.x + closest.w;
                if (screen_val.y - closest.y).abs() < threshold {
                    final_y = closest.y;
                }
            }
            // Snap A's right edge to B's left edge
            else if ((screen_val.x + screen_val.w) - closest.x).abs() < threshold {
                final_x = closest.x - screen_val.w;
                if (screen_val.y - closest.y).abs() < threshold {
                    final_y = closest.y;
                }
            }
            // Snap A's top edge to B's bottom edge
            else if (screen_val.y - (closest.y + closest.h)).abs() < threshold {
                final_y = closest.y + closest.h;
                if (screen_val.x - closest.x).abs() < threshold {
                    final_x = closest.x;
                }
            }
            // Snap A's bottom edge to B's top edge
            else if ((screen_val.y + screen_val.h) - closest.y).abs() < threshold {
                final_y = closest.y - screen_val.h;
                if (screen_val.x - closest.x).abs() < threshold {
                    final_x = closest.x;
                }
            }

            if let Some(s) = self.screens.get_mut(id) {
                s.x = final_x;
                s.y = final_y;
            }
        }
    }
}
