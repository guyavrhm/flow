#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod config;
pub mod crypto;
pub mod engine;
pub mod hardware;
pub mod network;
pub mod ui;
 
#[cfg(all(test, target_os = "macos"))]
pub mod mac_tests;

#[cfg(all(test, target_os = "linux"))]
pub mod linux_tests;

use crate::config::initialize_db;
use crate::engine::AppEngine;
use crate::ui::FlowApp;
use crate::ui::tray::SystemTrayManager;
use crate::hardware::init_keyboard_layout;
use std::sync::Arc;

fn main() {
    // Initialize platform keyboard layout cache on the main thread
    init_keyboard_layout();

    // 1. Initialize SQLite Database
    if let Err(e) = initialize_db() {
        eprintln!("Failed to initialize database: {:?}", e);
        std::process::exit(1);
    }

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = gtk::init() {
            eprintln!("Failed to initialize GTK: {:?}", e);
        }
    }


    // 2. Configure Eframe UI options
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("flow")
            .with_inner_size([780.0, 430.0])
            .with_resizable(false),
        ..Default::default()
    };

    // 3. Start egui run loop, initializing resources within winit's context
    if let Err(e) = eframe::run_native(
        "flow",
        options,
        Box::new(|cc| {
            // Instantiate AppEngine & SystemTray inside the winit application initialization context
            // to avoid NSApplication principal class conflict on macOS.
            let engine = Arc::new(AppEngine::new());
            engine.start();

            let tray = Arc::new(SystemTrayManager::new());

            Box::new(FlowApp::new(cc, engine, tray))
        }),
    ) {
        eprintln!("Failed to run egui application: {:?}", e);
    }
}
