use evdev::KeyCode;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyState {
    Press,
    Release,
}

impl KeyState {
    pub fn is_press(&self) -> bool {
        matches!(self, KeyState::Press)
    }

    pub fn to_i32(&self) -> i32 {
        match self {
            KeyState::Press => 1,
            KeyState::Release => 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", content = "data")]
pub enum EventType {
    RelativeMove {
        dx: i32,
        dy: i32,
    },
    MouseButton {
        code: u16,
        state: KeyState,
    },
    MouseWheel {
        delta: i32,
        #[serde(default)]
        horizontal: bool,
    },
    KeyboardKey {
        code: u16,
        state: KeyState,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MacroEvent {
    pub timestamp_ms: u64,
    pub event: EventType,
}

impl MacroEvent {
    pub fn new(timestamp_ms: u64, event: EventType) -> Self {
        Self {
            timestamp_ms,
            event,
        }
    }

    pub fn icon(&self) -> &'static str {
        match &self.event {
            EventType::RelativeMove { .. } => "✛",
            EventType::MouseButton { state, .. } => {
                if state.is_press() {
                    "▼"
                } else {
                    "▲"
                }
            }
            EventType::MouseWheel { .. } => "⇅",
            EventType::KeyboardKey { state, .. } => {
                if state.is_press() {
                    "⏷"
                } else {
                    "⏶"
                }
            }
        }
    }

    pub fn category_badge(&self) -> (&'static str, &'static str) {
        match &self.event {
            EventType::RelativeMove { .. } => ("MOVE", "rgba(59, 130, 246, 0.25)"),
            EventType::MouseButton { state, .. } => {
                if state.is_press() {
                    ("CLICK", "rgba(16, 185, 129, 0.25)")
                } else {
                    ("RELEASE", "rgba(239, 68, 68, 0.25)")
                }
            }
            EventType::MouseWheel { horizontal, .. } => {
                if *horizontal {
                    ("H-WHEEL", "rgba(168, 85, 247, 0.25)")
                } else {
                    ("WHEEL", "rgba(168, 85, 247, 0.25)")
                }
            }
            EventType::KeyboardKey { state, .. } => {
                if state.is_press() {
                    ("KEY-DN", "rgba(245, 158, 11, 0.25)")
                } else {
                    ("KEY-UP", "rgba(239, 68, 68, 0.25)")
                }
            }
        }
    }

    pub fn description(&self) -> String {
        match &self.event {
            EventType::RelativeMove { dx, dy } => {
                format!("Relative Move: dx = {:+4}, dy = {:+4}", dx, dy)
            }
            EventType::MouseButton { code, state } => {
                let name = key_code_to_name(*code);
                let action = match state {
                    KeyState::Press => "Pressed",
                    KeyState::Release => "Released",
                };
                format!("Mouse Button: {} [{}]", name, action)
            }
            EventType::MouseWheel { delta, horizontal } => {
                if *horizontal {
                    format!("Horizontal Scroll: {:+}", delta)
                } else {
                    format!("Vertical Scroll: {:+}", delta)
                }
            }
            EventType::KeyboardKey { code, state } => {
                let name = key_code_to_name(*code);
                let action = match state {
                    KeyState::Press => "Pressed",
                    KeyState::Release => "Released",
                };
                format!("Key: {} [{}]", name, action)
            }
        }
    }
}

/// Convert an evdev numeric keycode to a clean, user-friendly name
pub fn key_code_to_name(code: u16) -> String {
    match code {
        // Common Mouse Buttons
        272 => "Left Click (BTN_LEFT)".to_string(),
        273 => "Right Click (BTN_RIGHT)".to_string(),
        274 => "Middle Click (BTN_MIDDLE)".to_string(),
        275 => "Side Button (BTN_SIDE)".to_string(),
        276 => "Extra Button (BTN_EXTRA)".to_string(),
        277 => "Forward Button".to_string(),
        278 => "Back Button".to_string(),
        279 => "Task Button".to_string(),

        // Common Keyboard Keys
        1 => "Escape".to_string(),
        2 => "1".to_string(),
        3 => "2".to_string(),
        4 => "3".to_string(),
        5 => "4".to_string(),
        6 => "5".to_string(),
        7 => "6".to_string(),
        8 => "7".to_string(),
        9 => "8".to_string(),
        10 => "9".to_string(),
        11 => "0".to_string(),
        12 => "Minus (-)".to_string(),
        13 => "Equal (=)".to_string(),
        14 => "Backspace".to_string(),
        15 => "Tab".to_string(),
        16 => "Q".to_string(),
        17 => "W".to_string(),
        18 => "E".to_string(),
        19 => "R".to_string(),
        20 => "T".to_string(),
        21 => "Y".to_string(),
        22 => "U".to_string(),
        23 => "I".to_string(),
        24 => "O".to_string(),
        25 => "P".to_string(),
        28 => "Enter".to_string(),
        29 => "Left Ctrl".to_string(),
        30 => "A".to_string(),
        31 => "S".to_string(),
        32 => "D".to_string(),
        33 => "F".to_string(),
        34 => "G".to_string(),
        35 => "H".to_string(),
        36 => "J".to_string(),
        37 => "K".to_string(),
        38 => "L".to_string(),
        42 => "Left Shift".to_string(),
        44 => "Z".to_string(),
        45 => "X".to_string(),
        46 => "C".to_string(),
        47 => "V".to_string(),
        48 => "B".to_string(),
        49 => "N".to_string(),
        50 => "M".to_string(),
        54 => "Right Shift".to_string(),
        56 => "Left Alt".to_string(),
        57 => "Space".to_string(),
        58 => "Caps Lock".to_string(),
        59 => "F1".to_string(),
        60 => "F2".to_string(),
        61 => "F3".to_string(),
        62 => "F4".to_string(),
        63 => "F5".to_string(),
        64 => "F6".to_string(),
        65 => "F7".to_string(),
        66 => "F8".to_string(),
        67 => "F9".to_string(),
        68 => "F10".to_string(),
        70 => "Scroll Lock".to_string(),
        87 => "F11".to_string(),
        88 => "F12".to_string(),
        97 => "Right Ctrl".to_string(),
        100 => "Right Alt".to_string(),
        102 => "Home".to_string(),
        103 => "Up Arrow".to_string(),
        104 => "Page Up".to_string(),
        105 => "Left Arrow".to_string(),
        106 => "Right Arrow".to_string(),
        107 => "End".to_string(),
        108 => "Down Arrow".to_string(),
        109 => "Page Down".to_string(),
        110 => "Insert".to_string(),
        111 => "Delete".to_string(),
        119 => "Pause / Break".to_string(),
        125 => "Super/Meta".to_string(),
        other => {
            // Fallback to evdev KeyCode debug name
            let evdev_key = KeyCode::new(other);
            let s = format!("{:?}", evdev_key);
            if s.starts_with("KEY_") {
                s.trim_start_matches("KEY_").replace('_', " ")
            } else if s.starts_with("BTN_") {
                s.trim_start_matches("BTN_").replace('_', " ")
            } else {
                format!("Code {}", other)
            }
        }
    }
}
