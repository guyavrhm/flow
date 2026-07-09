use crate::config::{ScreenAttachments, get_screens};
use eframe::egui;
use std::collections::HashMap;

pub const DEFAULT_WIDTH: f32 = 120.0;
pub const DEFAULT_HEIGHT: f32 = 80.0;

#[derive(Clone, Debug)]
pub struct EditorScreen {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub is_connected: bool,
}

impl EditorScreen {
    pub fn center_x(&self) -> f32 {
        self.x + self.w / 2.0
    }

    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }

    pub fn distance(&self, other: &EditorScreen) -> f32 {
        ((self.center_x() - other.center_x()).powi(2)
            + (self.center_y() - other.center_y()).powi(2))
        .sqrt()
    }
}

pub struct ScreenLayoutCanvas {
    pub screens: HashMap<String, EditorScreen>,
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
        if let Ok(db_screens) = get_screens() {
            let mut screen_positions = HashMap::new();
            screen_positions.insert("main".to_string(), (200.0, 200.0));

            let mut queue = vec!["main".to_string()];
            let mut visited = std::collections::HashSet::new();
            visited.insert("main".to_string());

            while let Some(current_name) = queue.pop() {
                if let Some(&(curr_x, curr_y)) = screen_positions.get(&current_name) {
                    if let Some(db_scr) = db_screens.iter().find(|s| s.address == current_name) {
                        if let Some(ref t) = db_scr.top {
                            if !visited.contains(t) {
                                screen_positions
                                    .insert(t.clone(), (curr_x, curr_y - DEFAULT_HEIGHT));
                                visited.insert(t.clone());
                                queue.push(t.clone());
                            }
                        }
                        if let Some(ref b) = db_scr.bottom {
                            if !visited.contains(b) {
                                screen_positions
                                    .insert(b.clone(), (curr_x, curr_y + DEFAULT_HEIGHT));
                                visited.insert(b.clone());
                                queue.push(b.clone());
                            }
                        }
                        if let Some(ref l) = db_scr.left {
                            if !visited.contains(l) {
                                screen_positions
                                    .insert(l.clone(), (curr_x - DEFAULT_WIDTH, curr_y));
                                visited.insert(l.clone());
                                queue.push(l.clone());
                            }
                        }
                        if let Some(ref r) = db_scr.right {
                            if !visited.contains(r) {
                                screen_positions
                                    .insert(r.clone(), (curr_x + DEFAULT_WIDTH, curr_y));
                                visited.insert(r.clone());
                                queue.push(r.clone());
                            }
                        }
                    }
                }
            }

            for db_scr in db_screens {
                let name = db_scr.address;
                let is_connected = name == "main" || active_clients.contains(&name);
                let (x, y) = screen_positions.remove(&name).unwrap_or((50.0, 50.0));

                self.screens.insert(
                    name.clone(),
                    EditorScreen {
                        name,
                        x,
                        y,
                        w: DEFAULT_WIDTH,
                        h: DEFAULT_HEIGHT,
                        is_connected,
                    },
                );
            }
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(500.0, 400.0), egui::Sense::click_and_drag());

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);

        let mut dragged_screen = None;
        let mut drag_delta = egui::Vec2::ZERO;

        if response.dragged() {
            drag_delta = response.drag_delta();
        }

        for (name, screen) in self.screens.iter_mut() {
            let screen_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(screen.x, screen.y),
                egui::vec2(screen.w, screen.h),
            );

            if response.dragged()
                && ui
                    .input(|i| i.pointer.hover_pos())
                    .map_or(false, |pos| screen_rect.contains(pos))
            {
                if name != "main" {
                    dragged_screen = Some(name.clone());
                }
            }

            let is_selected = self.selected_screen.as_ref() == Some(name);

            let fill_color = if screen.is_connected {
                egui::Color32::from_rgb(140, 220, 140)
            } else {
                egui::Color32::from_rgb(190, 190, 190)
            };

            let stroke_color = if is_selected {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().widgets.active.bg_stroke.color
            };

            let stroke_width = if is_selected { 3.0 } else { 1.5 };

            painter.rect(
                screen_rect,
                4.0,
                fill_color,
                egui::Stroke::new(stroke_width, stroke_color),
            );

            let text_pos = screen_rect.center() - egui::vec2(15.0, 5.0);
            painter.text(
                text_pos,
                egui::Align2::LEFT_TOP,
                &screen.name,
                egui::FontId::proportional(14.0),
                egui::Color32::BLACK,
            );
        }

        if let Some(name) = dragged_screen {
            self.selected_screen = Some(name.clone());
            if let Some(screen) = self.screens.get_mut(&name) {
                screen.x += drag_delta.x;
                screen.y += drag_delta.y;
            }
        }

        if response.drag_released() {
            if let Some(name) = self.selected_screen.clone() {
                if name != "main" {
                    self.snap_screen(&name);
                }
            }
        }

        if response.clicked() {
            let mut clicked = None;
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                for (name, screen) in self.screens.iter() {
                    let screen_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(screen.x, screen.y),
                        egui::vec2(screen.w, screen.h),
                    );
                    if screen_rect.contains(pos) {
                        clicked = Some(name.clone());
                        break;
                    }
                }
            }
            self.selected_screen = clicked;
        }

        self.selected_screen.clone()
    }

    fn snap_screen(&mut self, name: &str) {
        let mut target = None;
        let mut min_dist = f32::MAX;

        let screen_val = match self.screens.get(name) {
            Some(s) => s.clone(),
            None => return,
        };

        for (other_name, other) in self.screens.iter() {
            if other_name != name {
                let d = screen_val.distance(other);
                if d < min_dist {
                    min_dist = d;
                    target = Some(other.clone());
                }
            }
        }

        if let Some(closest) = target {
            let relative_x = screen_val.center_x() - closest.center_x();
            let relative_y = screen_val.center_y() - closest.center_y();

            let mut final_x = screen_val.x;
            let mut final_y = screen_val.y;

            if relative_x.abs() > relative_y.abs() {
                if relative_x > 0.0 {
                    final_x = closest.x + closest.w;
                    final_y = closest.y;
                } else {
                    final_x = closest.x - screen_val.w;
                    final_y = closest.y;
                }
            } else {
                if relative_y > 0.0 {
                    final_x = closest.x;
                    final_y = closest.y + closest.h;
                } else {
                    final_x = closest.x;
                    final_y = closest.y - screen_val.h;
                }
            }

            if let Some(s) = self.screens.get_mut(name) {
                s.x = final_x;
                s.y = final_y;
            }
        }
    }

    pub fn compute_attachments(&self) -> HashMap<String, ScreenAttachments> {
        let mut results = HashMap::new();
        for (name, screen) in self.screens.iter() {
            let mut att = ScreenAttachments {
                address: name.clone(),
                top: None,
                right: None,
                bottom: None,
                left: None,
            };

            for (other_name, other) in self.screens.iter() {
                if other_name == name {
                    continue;
                }

                if (screen.x - other.x - other.w).abs() < 5.0 && (screen.y - other.y).abs() < 5.0 {
                    att.left = Some(other_name.clone());
                }
                if (other.x - screen.x - screen.w).abs() < 5.0 && (screen.y - other.y).abs() < 5.0 {
                    att.right = Some(other_name.clone());
                }
                if (screen.y - other.y - other.h).abs() < 5.0 && (screen.x - other.x).abs() < 5.0 {
                    att.top = Some(other_name.clone());
                }
                if (other.y - screen.y - screen.h).abs() < 5.0 && (screen.x - other.x).abs() < 5.0 {
                    att.bottom = Some(other_name.clone());
                }
            }

            results.insert(name.clone(), att);
        }
        results
    }
}
