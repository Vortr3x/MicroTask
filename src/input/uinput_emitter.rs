use crate::input::device_detector::VIRTUAL_DEVICE_NAME;
use crate::model::event::{EventType, KeyState};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, InputEvent, KeyCode, RelativeAxisCode};
use std::collections::HashSet;
use std::io;
use std::thread::sleep;
use std::time::Duration;
use tracing::{debug, error, info, warn};

pub struct UinputEmitter {
    device: VirtualDevice,
    pub pressed_keys: HashSet<u16>,
    pub pressed_mouse_buttons: HashSet<u16>,
}

impl UinputEmitter {
    /// Create and initialize the virtual uinput device with all required keys and axes
    pub fn new() -> Result<Self, io::Error> {
        let mut keys = AttributeSet::<KeyCode>::new();

        // Register standard keyboard keys (1 to 255)
        for code in 1..=255 {
            keys.insert(KeyCode::new(code));
        }

        // Register extended keyboard keys (e.g. multimedia, extra function keys up to 512)
        for code in 256..=512 {
            keys.insert(KeyCode::new(code));
        }

        // Register mouse buttons (BTN_LEFT, BTN_RIGHT, BTN_MIDDLE, BTN_SIDE, BTN_EXTRA, etc.)
        for code in 0x110..=0x11F {
            keys.insert(KeyCode::new(code));
        }

        // Register relative axes
        let mut rel_axes = AttributeSet::<RelativeAxisCode>::new();
        rel_axes.insert(RelativeAxisCode::REL_X);
        rel_axes.insert(RelativeAxisCode::REL_Y);
        rel_axes.insert(RelativeAxisCode::REL_WHEEL);
        rel_axes.insert(RelativeAxisCode::REL_HWHEEL);

        let device = VirtualDevice::builder()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .name(VIRTUAL_DEVICE_NAME)
            .with_keys(&keys)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .with_relative_axes(&rel_axes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
            .build()
            .map_err(|e| {
                error!("Failed to create uinput virtual device: {}", e);
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Failed to build VirtualDevice: {}. Check /dev/uinput permissions.", e),
                )
            })?;

        info!("UinputEmitter successfully created virtual device '{}'", VIRTUAL_DEVICE_NAME);

        Ok(Self {
            device,
            pressed_keys: HashSet::new(),
            pressed_mouse_buttons: HashSet::new(),
        })
    }

    /// Emits a single MacroEvent variant
    pub fn emit(&mut self, event: &EventType) -> Result<(), io::Error> {
        match event {
            EventType::RelativeMove { dx, dy } => {
                let mut batch = Vec::with_capacity(2);
                if *dx != 0 {
                    batch.push(InputEvent::new(
                        evdev::EventType::RELATIVE.0,
                        RelativeAxisCode::REL_X.0,
                        *dx,
                    ));
                }
                if *dy != 0 {
                    batch.push(InputEvent::new(
                        evdev::EventType::RELATIVE.0,
                        RelativeAxisCode::REL_Y.0,
                        *dy,
                    ));
                }
                if !batch.is_empty() {
                    self.device.emit(&batch)?;
                }
            }
            EventType::MouseButton { code, state } => {
                match state {
                    KeyState::Press => {
                        self.pressed_mouse_buttons.insert(*code);
                    }
                    KeyState::Release => {
                        self.pressed_mouse_buttons.remove(code);
                    }
                }
                let ev = InputEvent::new(evdev::EventType::KEY.0, *code, state.to_i32());
                self.device.emit(&[ev])?;
            }
            EventType::MouseWheel { delta, horizontal } => {
                let axis = if *horizontal {
                    RelativeAxisCode::REL_HWHEEL.0
                } else {
                    RelativeAxisCode::REL_WHEEL.0
                };
                let ev = InputEvent::new(evdev::EventType::RELATIVE.0, axis, *delta);
                self.device.emit(&[ev])?;
            }
            EventType::KeyboardKey { code, state } => {
                match state {
                    KeyState::Press => {
                        self.pressed_keys.insert(*code);
                    }
                    KeyState::Release => {
                        self.pressed_keys.remove(code);
                    }
                }
                let ev = InputEvent::new(evdev::EventType::KEY.0, *code, state.to_i32());
                self.device.emit(&[ev])?;
            }
        }
        Ok(())
    }

    /// Wayland Corner Anchor: Slams the pointer into the top-left corner (0, 0).
    /// This overcomes Wayland compositor relative limits by hitting the desktop bounds,
    /// creating a deterministic, calibrated starting point for relative playback.
    pub fn anchor_cursor_to_top_left(
        &mut self,
        bursts: u32,
        step: i32,
        delay_ms: u64,
    ) -> Result<(), io::Error> {
        debug!("Anchoring cursor to top-left corner (bursts: {}, step: {})", bursts, step);
        let move_x = InputEvent::new(evdev::EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, step);
        let move_y = InputEvent::new(evdev::EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, step);

        for _ in 0..bursts {
            self.device.emit(&[move_x, move_y])?;
            if delay_ms > 0 {
                sleep(Duration::from_millis(delay_ms));
            }
        }
        Ok(())
    }

    /// Wayland Coordinate Anchor: Slams the pointer into (0, 0), then advances
    /// to (target_x, target_y) using chunked relative moves to bypass pointer acceleration curves.
    pub fn anchor_cursor_to_coords(
        &mut self,
        target_x: i32,
        target_y: i32,
        bursts: u32,
        step: i32,
        delay_ms: u64,
    ) -> Result<(), io::Error> {
        self.anchor_cursor_to_top_left(bursts, step, delay_ms)?;

        if target_x <= 0 && target_y <= 0 {
            return Ok(());
        }

        // Brief pause after corner collision to stabilize compositor pointer position
        sleep(Duration::from_millis(15));

        let chunk_size = 40;
        let mut curr_x = 0;
        let mut curr_y = 0;

        while curr_x < target_x || curr_y < target_y {
            let dx = (target_x - curr_x).min(chunk_size).max(0);
            let dy = (target_y - curr_y).min(chunk_size).max(0);

            let mut batch = Vec::with_capacity(2);
            if dx > 0 {
                batch.push(InputEvent::new(
                    evdev::EventType::RELATIVE.0,
                    RelativeAxisCode::REL_X.0,
                    dx,
                ));
                curr_x += dx;
            }
            if dy > 0 {
                batch.push(InputEvent::new(
                    evdev::EventType::RELATIVE.0,
                    RelativeAxisCode::REL_Y.0,
                    dy,
                ));
                curr_y += dy;
            }

            if !batch.is_empty() {
                self.device.emit(&batch)?;
                sleep(Duration::from_millis(2));
            }
        }

        Ok(())
    }

    /// Release all currently held keys and mouse buttons.
    /// Vital for emergency stop / panic abort to prevent stuck keys!
    pub fn release_all_held(&mut self) {
        if self.pressed_keys.is_empty() && self.pressed_mouse_buttons.is_empty() {
            return;
        }

        warn!(
            "Emergency releasing {} held keys and {} held mouse buttons",
            self.pressed_keys.len(),
            self.pressed_mouse_buttons.len()
        );

        let keys_to_release: Vec<u16> = self.pressed_keys.drain().collect();
        for code in keys_to_release {
            let ev = InputEvent::new(evdev::EventType::KEY.0, code, 0);
            if let Err(e) = self.device.emit(&[ev]) {
                error!("Error releasing held key {}: {}", code, e);
            }
        }

        let buttons_to_release: Vec<u16> = self.pressed_mouse_buttons.drain().collect();
        for code in buttons_to_release {
            let ev = InputEvent::new(evdev::EventType::KEY.0, code, 0);
            if let Err(e) = self.device.emit(&[ev]) {
                error!("Error releasing held mouse button {}: {}", code, e);
            }
        }
    }
}

impl Drop for UinputEmitter {
    fn drop(&mut self) {
        self.release_all_held();
    }
}
