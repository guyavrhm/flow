use crate::hardware::get_resource_path;
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuItem, PredefinedMenuItem},
};

pub struct SystemTrayManager {
    tray_icon: Option<TrayIcon>,
    icon_connected: Icon,
    icon_disconnected: Icon,
    pub menu_settings_id: String,
    pub menu_help_id: String,
    pub menu_exit_id: String,
}

impl SystemTrayManager {
    pub fn new() -> Self {
        let icon_connected = load_icon_rgba("flowv.png");
        let icon_disconnected = load_icon_rgba("flowx.png");

        let menu = Menu::new();
        let menu_settings = MenuItem::new("Settings", true, None);
        let menu_help = MenuItem::new("Help", true, None);
        let menu_exit = MenuItem::new("Exit", true, None);

        menu.append(&menu_settings).unwrap();
        menu.append(&menu_help).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&menu_exit).unwrap();

        let menu_settings_id = menu_settings.id().0.clone();
        let menu_help_id = menu_help.id().0.clone();
        let menu_exit_id = menu_exit.id().0.clone();

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("flow")
            .with_icon(icon_disconnected.clone())
            .build()
            .ok();

        Self {
            tray_icon,
            icon_connected,
            icon_disconnected,
            menu_settings_id,
            menu_help_id,
            menu_exit_id,
        }
    }

    pub fn set_connected(&self) {
        if let Some(ref tray) = self.tray_icon {
            let _ = tray.set_icon(Some(self.icon_connected.clone()));
        }
    }

    pub fn set_disconnected(&self) {
        if let Some(ref tray) = self.tray_icon {
            let _ = tray.set_icon(Some(self.icon_disconnected.clone()));
        }
    }
}

fn load_icon_rgba(name: &str) -> Icon {
    let path = get_resource_path(name);
    match image::open(&path) {
        Ok(image) => {
            let rgba = image.to_rgba8();
            let (width, height) = rgba.dimensions();
            match Icon::from_rgba(rgba.into_raw(), width, height) {
                Ok(icon) => return icon,
                Err(e) => {
                    log::error!("Tray: Failed to parse icon from RGBA bytes for {}: {:?}", name, e);
                }
            }
        }
        Err(e) => {
            log::error!("Tray: Failed to open icon image at {:?}: {:?}", path, e);
        }
    }
    // Minimal transparent fallback icon
    log::warn!("Tray: Using transparent fallback icon for {}", name);
    Icon::from_rgba(vec![0u8; 16], 2, 2).unwrap()
}
