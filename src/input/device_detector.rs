use evdev::{enumerate, KeyCode, RelativeAxisCode};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

pub const VIRTUAL_DEVICE_NAME: &str = "MicroTask Virtual Input";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDeviceInfo {
    pub path: PathBuf,
    pub name: String,
    pub is_mouse: bool,
    pub is_keyboard: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionStatus {
    pub uinput_accessible: bool,
    pub dev_input_accessible: bool,
    pub in_input_group: bool,
    pub user_name: String,
    pub issues: Vec<String>,
}

impl PermissionStatus {
    pub fn is_all_ok(&self) -> bool {
        self.uinput_accessible && self.dev_input_accessible
    }
}

/// Detects all connected input devices and categorizes them into mice and keyboards
pub fn detect_devices() -> Vec<InputDeviceInfo> {
    let mut devices = Vec::new();

    for (path, device) in enumerate() {
        let name = device.name().unwrap_or("Unknown Device").to_string();

        // Skip our own virtual device
        if name == VIRTUAL_DEVICE_NAME {
            continue;
        }

        let mut is_mouse = false;
        let mut is_keyboard = false;

        // Check relative axes (typical for mice)
        if let Some(axes) = device.supported_relative_axes() {
            if axes.contains(RelativeAxisCode::REL_X) && axes.contains(RelativeAxisCode::REL_Y) {
                is_mouse = true;
            }
        }

        // Check key capabilities
        if let Some(keys) = device.supported_keys() {
            // Mouse buttons
            if keys.contains(KeyCode::BTN_LEFT)
                || keys.contains(KeyCode::BTN_RIGHT)
                || keys.contains(KeyCode::BTN_MIDDLE)
            {
                is_mouse = true;
            }

            // Keyboard keys
            if keys.contains(KeyCode::KEY_A)
                && keys.contains(KeyCode::KEY_SPACE)
                && keys.contains(KeyCode::KEY_ENTER)
            {
                is_keyboard = true;
            }
        }

        // Only register if it's a mouse or keyboard
        if is_mouse || is_keyboard {
            debug!("Detected input device: {} ({:?}) [Mouse: {}, Keyboard: {}]", name, path, is_mouse, is_keyboard);
            devices.push(InputDeviceInfo {
                path,
                name,
                is_mouse,
                is_keyboard,
                enabled: true, // enabled by default
            });
        }
    }

    // Sort stably by name then path
    devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    info!("Total physical input devices detected: {}", devices.len());
    devices
}

/// Check permissions for `/dev/uinput` and `/dev/input/*`
pub fn check_permissions() -> PermissionStatus {
    let mut issues = Vec::new();

    // Check /dev/uinput
    let uinput_path = Path::new("/dev/uinput");
    let uinput_accessible = if uinput_path.exists() {
        match OpenOptions::new().read(true).write(true).open(uinput_path) {
            Ok(_) => true,
            Err(e) => {
                issues.push(format!("Cannot open /dev/uinput (read/write error: {})", e));
                false
            }
        }
    } else {
        issues.push("/dev/uinput does not exist (kernel module 'uinput' not loaded)".to_string());
        false
    };

    // Check /dev/input/ event nodes
    let mut dev_input_accessible = false;
    let mut sample_count = 0;
    let mut error_count = 0;

    for (path, _) in enumerate() {
        sample_count += 1;
        match OpenOptions::new().read(true).open(&path) {
            Ok(_) => {
                dev_input_accessible = true;
            }
            Err(_) => {
                error_count += 1;
            }
        }
    }

    if sample_count == 0 {
        issues.push("No /dev/input/event* devices could be enumerated".to_string());
    } else if error_count > 0 && !dev_input_accessible {
        issues.push("Permission denied when reading /dev/input/event* devices".to_string());
    }

    // Check group membership
    let user_name = std::env::var("USER").unwrap_or_else(|_| "current user".to_string());
    let in_input_group = check_in_input_group();

    if !in_input_group {
        issues.push(format!("User '{}' does not appear to be in the 'input' group", user_name));
    }

    if !issues.is_empty() {
        warn!("Permission issues detected: {:?}", issues);
    } else {
        info!("All input & uinput permissions verified successfully!");
    }

    PermissionStatus {
        uinput_accessible,
        dev_input_accessible,
        in_input_group,
        user_name,
        issues,
    }
}

fn check_in_input_group() -> bool {
    // Read /proc/self/status or use libc::getgroups
    unsafe {
        let count = libc::getgroups(0, std::ptr::null_mut());
        if count <= 0 {
            return false;
        }
        let mut groups = vec![0 as libc::gid_t; count as usize];
        let ret = libc::getgroups(count, groups.as_mut_ptr());
        if ret < 0 {
            return false;
        }

        // Look up input group gid via libc::getgrnam
        let input_c_str = std::ffi::CString::new("input").unwrap();
        let grp = libc::getgrnam(input_c_str.as_ptr());
        if !grp.is_null() {
            let target_gid = (*grp).gr_gid;
            return groups.contains(&target_gid);
        }
    }
    // Fallback: if we were able to open /dev/uinput and event nodes, assume ok
    true
}
