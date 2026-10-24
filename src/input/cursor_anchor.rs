use crate::input::uinput_emitter::UinputEmitter;
use crate::model::config::{AnchorMethod, AnchorMode};
use crate::model::event::EventType;
use std::process::Command;
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, info, warn};

// Persistent virtual device singleton to prevent repeated udev/libinput re-registration cycles
static PERSISTENT_EMITTER: Mutex<Option<UinputEmitter>> = Mutex::new(None);

/// Ensures a persistent virtual uinput device is created and recognized by Wayland/libinput.
/// Calling this at app launch ensures zero latency when anchoring cursor later.
pub fn init_cursor_anchor() {
    let mut lock = PERSISTENT_EMITTER.lock().unwrap();
    if lock.is_none() {
        match UinputEmitter::new() {
            Ok(emitter) => {
                info!("Persistent anchor UinputEmitter created, warming up for Wayland compositor registration...");
                // Allow KWin/udev/libinput to discover and bind the virtual device to the desktop seat
                sleep(Duration::from_millis(350));
                *lock = Some(emitter);
                info!("Persistent anchor UinputEmitter ready");
            }
            Err(e) => {
                warn!("Failed to create persistent anchor UinputEmitter: {}", e);
            }
        }
    }
}

/// Detects the primary screen resolution on Wayland/Linux.
/// Queries kscreen-doctor (KDE Plasma) or falls back to standard 1920x1080.
pub fn detect_screen_resolution() -> (i32, i32) {
    if let Ok(output) = Command::new("kscreen-doctor").arg("-o").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("Geometry:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    for part in parts {
                        if part.contains('x') && !part.contains('@') {
                            let dims: Vec<&str> = part.split('x').collect();
                            if dims.len() == 2 {
                                if let (Ok(w), Ok(h)) = (dims[0].parse::<i32>(), dims[1].parse::<i32>()) {
                                    if w >= 640 && h >= 480 {
                                        debug!("Detected screen resolution from kscreen-doctor: {}x{}", w, h);
                                        return (w, h);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (1920, 1080)
}

/// Checks if ydotool is installed and responsive
pub fn is_ydotool_available() -> bool {
    Command::new("ydotool")
        .arg("--help")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Reads real-time desktop cursor coordinates directly from KWin (KDE Plasma 6 / Wayland)
pub fn query_kwin_cursor_pos() -> Option<(i32, i32)> {
    let script = "console.log('MICROC_POS:' + workspace.cursorPos.x + ':' + workspace.cursorPos.y);";
    let tmp = "/tmp/microtask_kwin_get_pos.js";
    let _ = std::fs::write(tmp, script);

    let _ = Command::new("qdbus")
        .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.loadScript", tmp, "microc_pos"])
        .output().ok()?;

    let _ = Command::new("qdbus")
        .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.start"])
        .output().ok()?;

    sleep(Duration::from_millis(25));

    let _ = Command::new("qdbus")
        .args(["org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting.unloadScript", "microc_pos"])
        .output().ok()?;

    let out = Command::new("journalctl")
        .args(["_COMM=kwin_wayland", "-n", "8", "--no-pager"])
        .output().ok()?;

    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().rev() {
        if let Some(idx) = line.find("MICROC_POS:") {
            let part = &line[idx + "MICROC_POS:".len()..];
            let coords: Vec<&str> = part.split(':').collect();
            if coords.len() >= 2 {
                if let (Ok(x), Ok(y)) = (coords[0].trim().parse::<i32>(), coords[1].trim().parse::<i32>()) {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

/// Positions the mouse cursor at the specified (target_x, target_y) coordinate.
/// Resolves Wayland restrictions via persistent virtual device and closed-loop compensation.
pub fn position_cursor(target_x: i32, target_y: i32, method: AnchorMethod) -> Result<(), String> {
    info!(
        "Positioning cursor to ({}, {}) using method {:?}",
        target_x, target_y, method
    );

    match method {
        AnchorMethod::Ydotool => run_ydotool_anchor(target_x, target_y),
        AnchorMethod::NativeUinput => run_native_uinput_anchor(target_x, target_y),
        AnchorMethod::Auto => {
            // In Auto mode: Native Uinput with persistent device and KWin closed-loop is most reliable
            match run_native_uinput_anchor(target_x, target_y) {
                Ok(()) => Ok(()),
                Err(err) => {
                    warn!("Native uinput anchor failed ({}), falling back to ydotool", err);
                    run_ydotool_anchor(target_x, target_y)
                }
            }
        }
    }
}

/// Executes relative ydotool commands (avoiding broken -a absolute flag on Wayland)
fn run_ydotool_anchor(target_x: i32, target_y: i32) -> Result<(), String> {
    // 1. Slam to top-left (0, 0) using massive negative relative offset
    let slam_output = Command::new("ydotool")
        .args(["mousemove", "-x", "-9999", "-y", "-9999"])
        .output()
        .map_err(|e| format!("Failed to spawn ydotool: {}", e))?;

    if !slam_output.status.success() {
        let stderr = String::from_utf8_lossy(&slam_output.stderr);
        return Err(format!("ydotool corner slam failed: {}", stderr.trim()));
    }

    if target_x <= 0 && target_y <= 0 {
        return Ok(());
    }

    sleep(Duration::from_millis(50));

    // 2. If KWin is present, use closed-loop feedback with ydotool relative moves
    if query_kwin_cursor_pos().is_some() {
        for _ in 0..7 {
            if let Some((cur_x, cur_y)) = query_kwin_cursor_pos() {
                let diff_x = target_x - cur_x;
                let diff_y = target_y - cur_y;
                if diff_x.abs() <= 2 && diff_y.abs() <= 2 {
                    return Ok(());
                }
                let dx = if diff_x.abs() <= 4 { diff_x } else { diff_x / 2 };
                let dy = if diff_y.abs() <= 4 { diff_y } else { diff_y / 2 };
                let _ = Command::new("ydotool")
                    .args(["mousemove", "-x", &dx.to_string(), "-y", &dy.to_string()])
                    .output();
                sleep(Duration::from_millis(25));
            }
        }
        return Ok(());
    }

    // Fallback: move relative accounting for ~2x standard pointer acceleration profile
    let step_x = target_x / 2;
    let step_y = target_y / 2;
    let move_output = Command::new("ydotool")
        .args(["mousemove", "-x", &step_x.to_string(), "-y", &step_y.to_string()])
        .output()
        .map_err(|e| format!("Failed to spawn ydotool offset: {}", e))?;

    if move_output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&move_output.stderr);
        Err(format!("ydotool offset failed: {}", stderr.trim()))
    }
}

/// Uses persistent uinput emitter with corner-slam and KWin closed-loop positioning
fn run_native_uinput_anchor(target_x: i32, target_y: i32) -> Result<(), String> {
    init_cursor_anchor();

    let mut lock = PERSISTENT_EMITTER.lock().unwrap();
    let emitter = match lock.as_mut() {
        Some(e) => e,
        None => return Err("Persistent UinputEmitter is not available".to_string()),
    };

    // If KWin is available on KDE Plasma 6 Wayland, use direct closed-loop positioning
    if query_kwin_cursor_pos().is_some() {
        debug!("Using KWin closed-loop cursor positioning toward ({}, {})", target_x, target_y);
        for _ in 0..7 {
            if let Some((cur_x, cur_y)) = query_kwin_cursor_pos() {
                let diff_x = target_x - cur_x;
                let diff_y = target_y - cur_y;
                if diff_x.abs() <= 2 && diff_y.abs() <= 2 {
                    return Ok(());
                }
                let dx = if diff_x.abs() <= 4 { diff_x } else { diff_x / 2 };
                let dy = if diff_y.abs() <= 4 { diff_y } else { diff_y / 2 };
                emitter
                    .emit(&EventType::RelativeMove { dx, dy })
                    .map_err(|e| format!("Uinput emit error: {}", e))?;
                sleep(Duration::from_millis(25));
            }
        }
        return Ok(());
    }

    // Standard fallback for non-KDE Wayland: slam corner then shift
    emitter
        .anchor_cursor_to_coords(target_x, target_y, 6, -1000, 5)
        .map_err(|e| format!("Native uinput corner anchor failed: {}", e))?;

    Ok(())
}

/// Resolves actual (x, y) coordinates for a given AnchorMode and configuration
pub fn resolve_anchor_coordinates(
    mode: AnchorMode,
    configured_x: i32,
    configured_y: i32,
) -> (i32, i32) {
    match mode {
        AnchorMode::TopLeft => (0, 0),
        AnchorMode::Center => (configured_x, configured_y),
        AnchorMode::Custom => (configured_x, configured_y),
        AnchorMode::Disabled => (0, 0),
    }
}
