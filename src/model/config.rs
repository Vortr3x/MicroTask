use super::event::MacroEvent;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorMode {
    /// Pure relative, no starting position synchronization
    Disabled,
    /// Calibrate to top-left corner (0, 0)
    TopLeft,
    /// Calibrate to screen center (e.g. 1280, 720)
    Center,
    /// Calibrate to custom coordinates (X, Y)
    Custom,
}

impl Default for AnchorMode {
    fn default() -> Self {
        AnchorMode::Center
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorMethod {
    /// Automatically try ydotool first, fall back to native uinput corner-slam
    Auto,
    /// Use ydotool (ydotool mousemove -a X Y)
    Ydotool,
    /// Use native Linux /dev/uinput corner-slam + relative shift
    NativeUinput,
}

impl Default for AnchorMethod {
    fn default() -> Self {
        AnchorMethod::Auto
    }
}

fn default_anchor_x() -> i32 {
    1280
}

fn default_anchor_y() -> i32 {
    720
}

fn default_true() -> bool {
    true
}

fn default_anchor_settle_delay_ms() -> u64 {
    100
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlaybackConfig {
    /// Playback speed multiplier (0.25x to 10.0x)
    pub speed_multiplier: f32,
    /// Number of times to replay (1..N)
    pub repeat_count: u32,
    /// Infinite loop mode
    pub infinite_repeat: bool,
    /// Delay between repetitions in milliseconds
    pub loop_delay_ms: u64,

    // Starting Cursor Anchor settings (Wayland start position synchronization)
    #[serde(default)]
    pub anchor_mode: AnchorMode,
    #[serde(default)]
    pub anchor_method: AnchorMethod,
    #[serde(default = "default_anchor_x")]
    pub anchor_x: i32,
    #[serde(default = "default_anchor_y")]
    pub anchor_y: i32,
    #[serde(default = "default_true")]
    pub anchor_on_record: bool,
    #[serde(default = "default_true")]
    pub anchor_on_playback: bool,
    #[serde(default = "default_anchor_settle_delay_ms")]
    pub anchor_settle_delay_ms: u64,

    /// Wayland corner anchor toggle: slam cursor into top-left corner before playback
    pub corner_anchor: bool,
    /// Number of burst steps for corner anchor
    pub anchor_burst_count: u32,
    /// Step distance per burst for corner anchor (negative value to go to top-left)
    pub anchor_burst_step: i32,
    /// Delay between corner anchor bursts in milliseconds
    pub anchor_burst_delay_ms: u64,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            speed_multiplier: 1.0,
            repeat_count: 1,
            infinite_repeat: false,
            loop_delay_ms: 100,
            anchor_mode: AnchorMode::Center,
            anchor_method: AnchorMethod::Auto,
            anchor_x: 1280,
            anchor_y: 720,
            anchor_on_record: true,
            anchor_on_playback: true,
            anchor_settle_delay_ms: 100,
            corner_anchor: false,
            anchor_burst_count: 6,
            anchor_burst_step: -1000,
            anchor_burst_delay_ms: 10,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HotkeyConfig {
    /// Global hotkey to toggle recording (default: F9 = 67)
    pub record_toggle_code: u16,
    /// Global hotkey to toggle playback (default: F10 = 68)
    pub play_toggle_code: u16,
    /// Global hotkey to abort / panic stop (default: Pause = 119, or F12 = 88)
    pub panic_abort_code: u16,
    /// Enable background keyboard daemon
    pub enabled: bool,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            record_toggle_code: 67, // KEY_F9
            play_toggle_code: 68,   // KEY_F10
            panic_abort_code: 119,  // KEY_PAUSE (safe from accidental Esc / F11 triggers)
            enabled: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MacroMetadata {
    pub name: String,
    pub created_at: String,
    pub description: String,
    pub version: u32,
    #[serde(default)]
    pub anchor_mode: Option<AnchorMode>,
    #[serde(default)]
    pub anchor_x: Option<i32>,
    #[serde(default)]
    pub anchor_y: Option<i32>,
}

impl Default for MacroMetadata {
    fn default() -> Self {
        Self {
            name: "New Macro".to_string(),
            created_at: "Unknown".to_string(),
            description: "Recorded with MicroTask (Wayland/Linux)".to_string(),
            version: 1,
            anchor_mode: Some(AnchorMode::Center),
            anchor_x: Some(1280),
            anchor_y: Some(720),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MacroData {
    pub metadata: MacroMetadata,
    pub events: Vec<MacroEvent>,
}

impl MacroData {
    pub fn new(name: impl Into<String>, events: Vec<MacroEvent>) -> Self {
        Self {
            metadata: MacroMetadata {
                name: name.into(),
                created_at: "Just now".to_string(),
                description: "Recorded with MicroTask".to_string(),
                version: 1,
                anchor_mode: Some(AnchorMode::Center),
                anchor_x: Some(1280),
                anchor_y: Some(720),
            },
            events,
        }
    }

    pub fn total_duration_ms(&self) -> u64 {
        self.events.last().map(|e| e.timestamp_ms).unwrap_or(0)
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn remove_event(&mut self, index: usize) {
        if index < self.events.len() {
            self.events.remove(index);
        }
    }

    /// Shifts timestamps so the first event starts at timestamp 0
    pub fn normalize_timestamps(&mut self) {
        if let Some(first) = self.events.first() {
            let offset = first.timestamp_ms;
            if offset > 0 {
                for ev in &mut self.events {
                    ev.timestamp_ms = ev.timestamp_ms.saturating_sub(offset);
                }
            }
        }
    }
}
