use std::path::PathBuf;

pub fn get_app_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            PathBuf::from(appdata).join("flow")
        } else {
            directories::UserDirs::new()
                .map(|u| u.home_dir().join(".flow"))
                .unwrap_or_else(|| PathBuf::from(".flow"))
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = directories::UserDirs::new() {
            home.home_dir().join(".flow")
        } else {
            PathBuf::from(".flow")
        }
    }
}

pub fn get_db_path() -> PathBuf {
    get_app_dir().join("flow.db")
}

pub fn get_log_path() -> PathBuf {
    get_app_dir().join("flow.log")
}
