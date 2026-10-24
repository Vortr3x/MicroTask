use microtask::input::device_detector::{check_permissions, detect_devices};
use microtask::input::uinput_emitter::UinputEmitter;
use microtask::model::config::{MacroData, PlaybackConfig};
use microtask::model::event::{key_code_to_name, EventType, KeyState, MacroEvent};
use microtask::storage::serializer::{load_macro_from_file, save_macro_to_file};
use std::fs;

#[test]
fn test_event_serialization_roundtrip() {
    let events = vec![
        MacroEvent::new(0, EventType::RelativeMove { dx: 10, dy: -5 }),
        MacroEvent::new(
            50,
            EventType::MouseButton {
                code: 272,
                state: KeyState::Press,
            },
        ),
        MacroEvent::new(
            100,
            EventType::MouseButton {
                code: 272,
                state: KeyState::Release,
            },
        ),
        MacroEvent::new(
            150,
            EventType::MouseWheel {
                delta: 1,
                horizontal: false,
            },
        ),
        MacroEvent::new(
            200,
            EventType::KeyboardKey {
                code: 30, // KEY_A
                state: KeyState::Press,
            },
        ),
        MacroEvent::new(
            250,
            EventType::KeyboardKey {
                code: 30,
                state: KeyState::Release,
            },
        ),
    ];

    let macro_data = MacroData::new("Test Macro", events.clone());
    let temp_file = std::env::temp_dir().join("microtask_test_macro.json");

    // Save
    assert!(save_macro_to_file(&temp_file, &macro_data).is_ok());

    // Load
    let loaded = load_macro_from_file(&temp_file).expect("Failed to load saved macro");
    assert_eq!(loaded.events.len(), events.len());
    assert_eq!(loaded.metadata.name, "Test Macro");
    assert_eq!(loaded.events[0].event, EventType::RelativeMove { dx: 10, dy: -5 });

    let _ = fs::remove_file(temp_file);
}

#[test]
fn test_key_code_to_name_translation() {
    assert_eq!(key_code_to_name(272), "Left Click (BTN_LEFT)");
    assert_eq!(key_code_to_name(273), "Right Click (BTN_RIGHT)");
    assert_eq!(key_code_to_name(1), "Escape");
    assert_eq!(key_code_to_name(30), "A");
    assert_eq!(key_code_to_name(57), "Space");
    assert_eq!(key_code_to_name(67), "F9");
    assert_eq!(key_code_to_name(68), "F10");
    assert_eq!(key_code_to_name(87), "F11");
}

#[test]
fn test_timestamp_normalization() {
    let mut macro_data = MacroData::new(
        "Offset Macro",
        vec![
            MacroEvent::new(1000, EventType::RelativeMove { dx: 1, dy: 1 }),
            MacroEvent::new(1050, EventType::RelativeMove { dx: 2, dy: 2 }),
            MacroEvent::new(1200, EventType::RelativeMove { dx: 3, dy: 3 }),
        ],
    );

    macro_data.normalize_timestamps();
    assert_eq!(macro_data.events[0].timestamp_ms, 0);
    assert_eq!(macro_data.events[1].timestamp_ms, 50);
    assert_eq!(macro_data.events[2].timestamp_ms, 200);
}

#[test]
fn test_device_detection() {
    let devices = detect_devices();
    println!("Detected {} physical input devices", devices.len());
    for dev in &devices {
        println!("- {} ({:?}) [Mouse: {}, Keyboard: {}]", dev.name, dev.path, dev.is_mouse, dev.is_keyboard);
    }
    // We saw in earlier checks that keyboard & mouse are connected
    assert!(!devices.is_empty(), "Should detect at least one input device");
}

#[test]
fn test_permissions_check() {
    let status = check_permissions();
    println!("Permissions status: {:?}", status);
    assert!(status.uinput_accessible, "/dev/uinput should be accessible");
    assert!(status.dev_input_accessible, "/dev/input/* should be accessible");
}

#[test]
fn test_uinput_virtual_device_creation() {
    let emitter = UinputEmitter::new();
    assert!(emitter.is_ok(), "UinputEmitter should build successfully");
}

#[test]
fn test_playback_config_defaults() {
    let cfg = PlaybackConfig::default();
    assert_eq!(cfg.speed_multiplier, 1.0);
    assert_eq!(cfg.repeat_count, 1);
    assert!(!cfg.infinite_repeat);
    assert!(!cfg.corner_anchor);
    assert!(cfg.anchor_on_record);
    assert!(cfg.anchor_on_playback);
    assert_eq!(cfg.anchor_mode, microtask::model::config::AnchorMode::Center);
}

#[test]
fn test_cursor_anchor_resolution_and_metadata() {
    use microtask::input::cursor_anchor::resolve_anchor_coordinates;
    use microtask::model::config::AnchorMode;

    assert_eq!(resolve_anchor_coordinates(AnchorMode::TopLeft, 100, 200), (0, 0));
    assert_eq!(resolve_anchor_coordinates(AnchorMode::Center, 1280, 720), (1280, 720));
    assert_eq!(resolve_anchor_coordinates(AnchorMode::Custom, 450, 600), (450, 600));
    assert_eq!(resolve_anchor_coordinates(AnchorMode::Disabled, 450, 600), (0, 0));

    // Test serialization of anchor metadata in macro
    let mut data = MacroData::new("Anchor Test", vec![]);
    data.metadata.anchor_mode = Some(AnchorMode::Custom);
    data.metadata.anchor_x = Some(777);
    data.metadata.anchor_y = Some(888);

    let temp_file = std::env::temp_dir().join("microtask_anchor_test.json");
    assert!(save_macro_to_file(&temp_file, &data).is_ok());

    let loaded = load_macro_from_file(&temp_file).expect("Failed to load");
    assert_eq!(loaded.metadata.anchor_mode, Some(AnchorMode::Custom));
    assert_eq!(loaded.metadata.anchor_x, Some(777));
    assert_eq!(loaded.metadata.anchor_y, Some(888));

    let _ = fs::remove_file(temp_file);
}

#[test]
fn test_app_settings_persistence() {
    use microtask::model::config::{AnchorMethod, AnchorMode};
    use microtask::storage::config_storage::AppSettings;

    let mut settings = AppSettings::default();
    settings.playback.speed_multiplier = 2.5;
    settings.playback.repeat_count = 42;
    settings.playback.anchor_mode = AnchorMode::Custom;
    settings.playback.anchor_method = AnchorMethod::NativeUinput;
    settings.playback.anchor_x = 999;
    settings.playback.anchor_y = 888;
    settings.hotkeys.record_toggle_code = 68;
    settings.disabled_device_names = vec!["USB Optical Mouse".to_string()];

    let temp_file = std::env::temp_dir().join("microtask_test_settings.json");

    // Test write
    let file = fs::File::create(&temp_file).expect("Failed to create temp settings file");
    let writer = std::io::BufWriter::new(file);
    assert!(serde_json::to_writer_pretty(writer, &settings).is_ok());

    // Test read
    let file = fs::File::open(&temp_file).expect("Failed to open temp settings file");
    let reader = std::io::BufReader::new(file);
    let loaded: AppSettings = serde_json::from_reader(reader).expect("Failed to parse settings");

    assert_eq!(loaded.playback.speed_multiplier, 2.5);
    assert_eq!(loaded.playback.repeat_count, 42);
    assert_eq!(loaded.playback.anchor_mode, AnchorMode::Custom);
    assert_eq!(loaded.playback.anchor_method, AnchorMethod::NativeUinput);
    assert_eq!(loaded.playback.anchor_x, 999);
    assert_eq!(loaded.playback.anchor_y, 888);
    assert_eq!(loaded.hotkeys.record_toggle_code, 68);
    assert_eq!(loaded.disabled_device_names, vec!["USB Optical Mouse".to_string()]);

    let _ = fs::remove_file(temp_file);
}


