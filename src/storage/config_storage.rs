use crate::model::config::{HotkeyConfig, PlaybackConfig};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AppSettings {
    pub playback: PlaybackConfig,
    pub hotkeys: HotkeyConfig,
    #[serde(default)]
    pub disabled_device_names: Vec<String>,
}

/// Returns the standard XDG path for settings: ~/.config/microtask/settings.json
pub fn get_settings_path() -> PathBuf {
    let base_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
        })
        .unwrap_or_else(|| PathBuf::from("."));

    base_dir.join("microtask").join("settings.json")
}

/// Loads app settings from ~/.config/microtask/settings.json, or returns None if not found or invalid
pub fn load_app_settings() -> Option<AppSettings> {
    let path = get_settings_path();
    if !path.exists() {
        debug!("Settings file does not exist at {:?}", path);
        return None;
    }

    match File::open(&path) {
        Ok(file) => {
            let reader = BufReader::new(file);
            match serde_json::from_reader(reader) {
                Ok(settings) => {
                    info!("Successfully loaded app settings from {:?}", path);
                    Some(settings)
                }
                Err(e) => {
                    warn!("Failed to parse settings JSON from {:?}: {}", path, e);
                    None
                }
            }
        }
        Err(e) => {
            warn!("Failed to open settings file {:?}: {}", path, e);
            None
        }
    }
}

/// Saves app settings to ~/.config/microtask/settings.json
pub fn save_app_settings(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            error!("Failed to create settings directory {:?}: {}", parent, e);
            return Err(format!("Failed to create config directory: {}", e));
        }
    }

    let file = File::create(&path).map_err(|e| {
        error!("Failed to create settings file {:?}: {}", path, e);
        format!("Failed to create settings file: {}", e)
    })?;

    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, settings).map_err(|e| {
        error!("Failed to serialize settings JSON: {}", e);
        format!("Failed to serialize settings: {}", e)
    })?;

    info!("Saved app settings to {:?}", path);
    Ok(())
}
