#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use flow::config::initialize_db;
use flow::engine::AppEngine;
use flow::ui::FlowApp;
use flow::ui::tray::SystemTrayManager;
use flow::hardware::init_keyboard_layout;
use std::sync::Arc;

fn main() {
    if let Ok(log_path) = flow::logger::setup_logging() {
        log::info!("Logging initialized successfully. Logs written to {:?}", log_path);
    } else {
        eprintln!("Failed to initialize logging.");
    }

    // Initialize platform keyboard layout cache on the main thread
    init_keyboard_layout();

    // Initialize SQLite Database
    if let Err(e) = initialize_db() {
        log::error!("Failed to initialize database: {:?}", e);
        std::process::exit(1);
    }

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = gtk::init() {
            log::error!("Failed to initialize GTK: {:?}", e);
        }
    }


    // Configure Eframe UI options
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("flow")
            .with_inner_size([780.0, 430.0])
            .with_resizable(false),
        ..Default::default()
    };

    // Start egui run loop, initializing resources within winit's context
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
        log::error!("Failed to run egui application: {:?}", e);
    }
}
