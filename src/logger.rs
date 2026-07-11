use crate::paths::get_log_path;
use simplelog::{ColorChoice, CombinedLogger, LevelFilter, TermLogger, TerminalMode, WriteLogger};
use std::path::PathBuf;

pub fn setup_logging() -> Result<PathBuf, String> {
    let log_path = get_log_path();
    
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create log directory: {:?}", e))?;
    }
    
    // Rotate log file if it exceeds 10MB to prevent unbounded disk usage
    if let Ok(metadata) = std::fs::metadata(&log_path) {
        if metadata.len() > 10 * 1024 * 1024 {
            let backup_path = log_path.with_extension("log.old");
            let _ = std::fs::rename(&log_path, &backup_path);
        }
    }
    
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("Failed to open log file: {:?}", e))?;
    
    let env_level = std::env::var("FLOW_LOG")
        .ok()
        .and_then(|val| {
            match val.to_lowercase().as_str() {
                "off" => Some(LevelFilter::Off),
                "error" => Some(LevelFilter::Error),
                "warn" => Some(LevelFilter::Warn),
                "info" => Some(LevelFilter::Info),
                "debug" => Some(LevelFilter::Debug),
                "trace" => Some(LevelFilter::Trace),
                _ => None,
            }
        });

    let term_level = env_level.unwrap_or(if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    });

    let file_level = env_level.unwrap_or(if cfg!(debug_assertions) {
        LevelFilter::Trace
    } else {
        LevelFilter::Debug
    });

    let config = simplelog::ConfigBuilder::new()
        .add_filter_allow_str("flow")
        .build();

    CombinedLogger::init(vec![
        TermLogger::new(
            term_level,
            config.clone(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        ),
        WriteLogger::new(
            file_level,
            config,
            file,
        ),
    ])
    .map_err(|e| format!("Failed to initialize logging framework: {:?}", e))?;
    
    Ok(log_path)
}
