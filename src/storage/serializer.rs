use crate::model::config::MacroData;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use tracing::{error, info};

pub fn save_macro_to_file(path: &Path, data: &MacroData) -> Result<(), String> {
    let file = File::create(path).map_err(|e| {
        error!("Failed to create macro file {:?}: {}", path, e);
        format!("Failed to create file: {}", e)
    })?;

    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, data).map_err(|e| {
        error!("Failed to serialize macro to JSON: {}", e);
        format!("Failed to serialize macro JSON: {}", e)
    })?;

    info!("Saved macro with {} events to {:?}", data.events.len(), path);
    Ok(())
}

pub fn load_macro_from_file(path: &Path) -> Result<MacroData, String> {
    let file = File::open(path).map_err(|e| {
        error!("Failed to open macro file {:?}: {}", path, e);
        format!("Failed to open file: {}", e)
    })?;

    let reader = BufReader::new(file);
    let mut data: MacroData = serde_json::from_reader(reader).map_err(|e| {
        error!("Failed to parse macro JSON from {:?}: {}", path, e);
        format!("Invalid macro JSON file: {}", e)
    })?;

    // Validate and normalize
    data.normalize_timestamps();
    info!("Loaded macro '{}' with {} events from {:?}", data.metadata.name, data.events.len(), path);
    Ok(data)
}
