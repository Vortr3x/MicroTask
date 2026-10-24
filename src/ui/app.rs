use crate::input::cursor_anchor::{
    detect_screen_resolution, init_cursor_anchor, is_ydotool_available, position_cursor,
    resolve_anchor_coordinates,
};
use crate::input::device_detector::{check_permissions, detect_devices, InputDeviceInfo, PermissionStatus};
use crate::input::hotkeys::{HotkeyAction, HotkeyDaemon, HotkeyDaemonHandle};
use crate::input::player::{MacroPlayer, PlaybackProgress, PlayerHandle};
use crate::input::recorder::{InputRecorder, RecorderHandle};
use crate::model::config::{AnchorMethod, AnchorMode, HotkeyConfig, MacroData, PlaybackConfig};
use crate::model::event::{EventType, KeyState, MacroEvent};
use crate::storage::config_storage::{
    get_settings_path, load_app_settings, save_app_settings, AppSettings,
};
use crate::storage::serializer::{load_macro_from_file, save_macro_to_file};
use crate::ui::components::{action_button, event_row, stat_card, status_pill};
use eframe::egui::{self, Color32, RichText, ScrollArea, Stroke, Ui};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppTab {
    Events,
    Settings,
    Devices,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    Idle,
    Recording,
    Playing,
    Paused,
}

pub struct MicroTaskApp {
    // Macro data
    pub macro_data: MacroData,
    pub playback_config: PlaybackConfig,
    pub hotkey_config: HotkeyConfig,

    // Hardware & system state
    pub detected_devices: Vec<InputDeviceInfo>,
    pub permission_status: PermissionStatus,

    // Runtime state
    pub state: EngineState,
    pub current_tab: AppTab,
    pub recording_start: Option<Instant>,
    pub recording_duration_ms: u64,
    pub playback_progress: Option<PlaybackProgress>,
    pub notification_banner: Option<(String, bool, Instant)>, // (message, is_error, timestamp)
    pub selected_event_idx: Option<usize>,

    // Channels & Handles
    recorder_handle: Option<RecorderHandle>,
    player_handle: Option<PlayerHandle>,
    _hotkey_handle: Option<HotkeyDaemonHandle>,

    event_rx: crossbeam_channel::Receiver<MacroEvent>,
    event_tx: crossbeam_channel::Sender<MacroEvent>,

    progress_rx: crossbeam_channel::Receiver<PlaybackProgress>,
    progress_tx: crossbeam_channel::Sender<PlaybackProgress>,

    hotkey_rx: crossbeam_channel::Receiver<HotkeyAction>,
}

impl MicroTaskApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Configure custom sleek dark visuals
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 20, 28);
        visuals.window_fill = Color32::from_rgb(22, 25, 34);
        visuals.selection.bg_fill = Color32::from_rgb(37, 99, 235);
        visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(96, 165, 250));
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(24, 27, 36);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 34, 46);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 46, 62);
        visuals.widgets.active.bg_fill = Color32::from_rgb(49, 57, 77);
        cc.egui_ctx.set_visuals(visuals);

        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        let (hotkey_tx, hotkey_rx) = crossbeam_channel::unbounded();

        // Warm up persistent uinput anchor device in background
        std::thread::spawn(init_cursor_anchor);

        // Detect screen resolution
        let (scr_w, scr_h) = detect_screen_resolution();

        // Load persisted settings from disk if available, otherwise initialize defaults
        let (playback_config, hotkey_config, disabled_devices) = if let Some(settings) = load_app_settings() {
            info!("Restored application settings from disk");
            (settings.playback, settings.hotkeys, settings.disabled_device_names)
        } else {
            info!("No saved settings found, initializing default configuration");
            let mut default_pb = PlaybackConfig::default();
            default_pb.anchor_x = scr_w / 2;
            default_pb.anchor_y = scr_h / 2;
            (default_pb, HotkeyConfig::default(), Vec::new())
        };

        // Start global hotkey daemon with loaded hotkey configuration
        let hotkey_handle = match HotkeyDaemon::start(hotkey_config.clone(), hotkey_tx) {
            Ok(h) => {
                info!("Global hotkey daemon initialized successfully");
                Some(h)
            }
            Err(e) => {
                warn!("Failed to initialize global hotkey daemon: {}", e);
                None
            }
        };

        let mut detected_devices = detect_devices();
        for dev in &mut detected_devices {
            if disabled_devices.contains(&dev.name) {
                dev.enabled = false;
            }
        }
        let permission_status = check_permissions();

        let mut macro_data = MacroData::default();
        macro_data.metadata.anchor_x = Some(playback_config.anchor_x);
        macro_data.metadata.anchor_y = Some(playback_config.anchor_y);

        Self {
            macro_data,
            playback_config,
            hotkey_config,
            detected_devices,
            permission_status,
            state: EngineState::Idle,
            current_tab: AppTab::Events,
            recording_start: None,
            recording_duration_ms: 0,
            playback_progress: None,
            notification_banner: None,
            selected_event_idx: None,
            recorder_handle: None,
            player_handle: None,
            _hotkey_handle: hotkey_handle,
            event_rx,
            event_tx,
            progress_rx,
            progress_tx,
            hotkey_rx,
        }
    }

    pub fn set_notification(&mut self, msg: impl Into<String>, is_error: bool) {
        self.notification_banner = Some((msg.into(), is_error, Instant::now()));
    }

    pub fn get_anchor_coordinates(&self) -> (i32, i32) {
        resolve_anchor_coordinates(
            self.playback_config.anchor_mode,
            self.playback_config.anchor_x,
            self.playback_config.anchor_y,
        )
    }

    /// Persists current application settings, hotkeys, and device toggles to ~/.config/microtask/settings.json
    pub fn save_settings(&self) {
        let disabled_names: Vec<String> = self
            .detected_devices
            .iter()
            .filter(|d| !d.enabled)
            .map(|d| d.name.clone())
            .collect();

        let settings = AppSettings {
            playback: self.playback_config.clone(),
            hotkeys: self.hotkey_config.clone(),
            disabled_device_names: disabled_names,
        };

        if let Err(e) = save_app_settings(&settings) {
            warn!("Failed to save app settings: {}", e);
        }
    }

    /// Resets all settings to factory defaults
    pub fn reset_to_defaults(&mut self) {
        let (scr_w, scr_h) = detect_screen_resolution();
        let mut pb = PlaybackConfig::default();
        pb.anchor_x = scr_w / 2;
        pb.anchor_y = scr_h / 2;
        self.playback_config = pb;
        self.hotkey_config = HotkeyConfig::default();
        for dev in &mut self.detected_devices {
            dev.enabled = true;
        }
        self.save_settings();
        self.set_notification("Settings reset to factory defaults and saved", false);
    }

    /// Toggles recording on/off
    pub fn toggle_recording(&mut self) {
        if self.state == EngineState::Recording {
            self.stop_recording();
        } else {
            self.start_recording();
        }
    }

    pub fn start_recording(&mut self) {
        if self.state == EngineState::Playing {
            self.stop_playback();
        }

        // Get enabled device paths
        let paths: Vec<PathBuf> = self
            .detected_devices
            .iter()
            .filter(|d| d.enabled)
            .map(|d| d.path.clone())
            .collect();

        if paths.is_empty() {
            self.set_notification("Cannot record: no devices selected or enabled", true);
            return;
        }

        // Synchronize starting position: move cursor to anchor before recording begins
        let mut anchor_notice = String::new();
        if self.playback_config.anchor_on_record && self.playback_config.anchor_mode != AnchorMode::Disabled {
            let (target_x, target_y) = self.get_anchor_coordinates();
            info!("Anchoring cursor to ({}, {}) before recording starts", target_x, target_y);
            if let Err(e) = position_cursor(target_x, target_y, self.playback_config.anchor_method) {
                warn!("Cursor anchor before recording failed: {}", e);
            }
            if self.playback_config.anchor_settle_delay_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(self.playback_config.anchor_settle_delay_ms));
            }
            anchor_notice = format!(" [Cursor anchored at ({}, {})]", target_x, target_y);
            self.macro_data.metadata.anchor_mode = Some(self.playback_config.anchor_mode);
            self.macro_data.metadata.anchor_x = Some(target_x);
            self.macro_data.metadata.anchor_y = Some(target_y);
        } else {
            self.macro_data.metadata.anchor_mode = Some(AnchorMode::Disabled);
            self.macro_data.metadata.anchor_x = None;
            self.macro_data.metadata.anchor_y = None;
        }

        // Clear existing events for fresh recording
        self.macro_data.clear();
        self.recording_start = Some(Instant::now());
        self.recording_duration_ms = 0;

        match InputRecorder::start(
            Some(paths),
            Some(self.event_tx.clone()),
            Some(self.hotkey_config.record_toggle_code),
        ) {
            Ok(handle) => {
                self.recorder_handle = Some(handle);
                self.state = EngineState::Recording;
                self.set_notification(format!("Recording started...{} Press F9 to stop", anchor_notice), false);
                info!("Recording started");
            }
            Err(e) => {
                self.set_notification(format!("Failed to start recording: {}", e), true);
                error!("Recording failed to start: {}", e);
            }
        }
    }

    pub fn stop_recording(&mut self) {
        if let Some(handle) = self.recorder_handle.take() {
            let events = handle.stop();
            self.macro_data.events = events;
            self.macro_data.normalize_timestamps();
            self.state = EngineState::Idle;
            self.recording_start = None;
            let anchor_info = if let (Some(x), Some(y)) = (self.macro_data.metadata.anchor_x, self.macro_data.metadata.anchor_y) {
                format!(" Anchor: ({}, {}).", x, y)
            } else {
                String::new()
            };
            self.set_notification(
                format!("Recording stopped. Saved {} events.{}", self.macro_data.events.len(), anchor_info),
                false,
            );
            info!("Recording stopped: {} events saved", self.macro_data.events.len());
        }
    }

    /// Toggles playback on/pause/stop
    pub fn toggle_playback(&mut self) {
        match self.state {
            EngineState::Idle => self.start_playback(),
            EngineState::Playing => self.pause_playback(),
            EngineState::Paused => self.resume_playback(),
            EngineState::Recording => {
                self.stop_recording();
                self.start_playback();
            }
        }
    }

    pub fn start_playback(&mut self) {
        if self.macro_data.is_empty() {
            self.set_notification("Cannot play: macro buffer is empty! Record or load a macro first.", true);
            return;
        }

        if self.state == EngineState::Recording {
            self.stop_recording();
        }

        let events = self.macro_data.events.clone();
        let mut config = self.playback_config.clone();

        // If macro has a recorded anchor and config hasn't disabled anchor, use coordinates
        if config.anchor_on_playback && config.anchor_mode != AnchorMode::Disabled {
            if let (Some(x), Some(y)) = (self.macro_data.metadata.anchor_x, self.macro_data.metadata.anchor_y) {
                config.anchor_x = x;
                config.anchor_y = y;
            }
        }

        match MacroPlayer::play(events, config, Some(self.progress_tx.clone())) {
            Ok(handle) => {
                self.player_handle = Some(handle);
                self.state = EngineState::Playing;
                let anchor_str = if self.playback_config.anchor_on_playback && self.playback_config.anchor_mode != AnchorMode::Disabled {
                    let (x, y) = self.get_anchor_coordinates();
                    format!(" [Anchored at ({}, {})]", x, y)
                } else {
                    String::new()
                };
                self.set_notification(format!("Macro playback started.{} Press F11 or Esc to abort.", anchor_str), false);
                info!("Playback started");
            }
            Err(e) => {
                self.set_notification(format!("Failed to start playback: {}", e), true);
                error!("Playback start failed: {}", e);
            }
        }
    }

    pub fn pause_playback(&mut self) {
        if let Some(ref handle) = self.player_handle {
            handle.pause();
            self.state = EngineState::Paused;
            self.set_notification("Playback paused. Press F10 to resume.", false);
        }
    }

    pub fn resume_playback(&mut self) {
        if let Some(ref handle) = self.player_handle {
            handle.resume();
            self.state = EngineState::Playing;
            self.set_notification("Playback resumed.", false);
        }
    }

    pub fn stop_playback(&mut self) {
        if let Some(handle) = self.player_handle.take() {
            handle.stop();
            self.state = EngineState::Idle;
            self.playback_progress = None;
            self.set_notification("Playback stopped.", false);
        }
    }

    pub fn emergency_panic_stop(&mut self) {
        warn!("EMERGENCY PANIC STOP triggered!");
        if self.state == EngineState::Recording {
            self.stop_recording();
        }
        if self.state == EngineState::Playing || self.state == EngineState::Paused {
            self.stop_playback();
        }
        self.state = EngineState::Idle;
        self.set_notification("EMERGENCY STOPPED! All keys & mouse buttons released.", true);
    }

    /// Process incoming background channels (recorder events, player progress, hotkeys)
    fn process_channels(&mut self) {
        // 1. Process hotkey daemon events
        while let Ok(action) = self.hotkey_rx.try_recv() {
            match action {
                HotkeyAction::RecordToggle => self.toggle_recording(),
                HotkeyAction::PlayToggle => self.toggle_playback(),
                HotkeyAction::PanicStop => self.emergency_panic_stop(),
            }
        }

        // 2. Process live recorded events
        while let Ok(event) = self.event_rx.try_recv() {
            self.macro_data.events.push(event);
        }

        // 3. Process playback progress updates
        while let Ok(prog) = self.progress_rx.try_recv() {
            if prog.is_finished {
                self.state = EngineState::Idle;
                self.player_handle = None;
                self.playback_progress = None;
                self.set_notification("Macro playback completed.", false);
            } else {
                self.playback_progress = Some(prog);
            }
        }

        // Update recording duration
        if let Some(start) = self.recording_start {
            self.recording_duration_ms = start.elapsed().as_millis() as u64;
        }
    }

    /// Generate a demo square movement macro for instant testing
    pub fn load_sample_demo_macro(&mut self) {
        let mut events = Vec::new();
        let mut t: u64 = 0;

        // Move in a square: Right, Down, Left, Up
        for _ in 0..15 {
            t += 16;
            events.push(MacroEvent::new(t, EventType::RelativeMove { dx: 15, dy: 0 }));
        }
        t += 80;
        events.push(MacroEvent::new(t, EventType::MouseButton { code: 272, state: KeyState::Press }));
        t += 50;
        events.push(MacroEvent::new(t, EventType::MouseButton { code: 272, state: KeyState::Release }));

        for _ in 0..15 {
            t += 16;
            events.push(MacroEvent::new(t, EventType::RelativeMove { dx: 0, dy: 15 }));
        }
        t += 80;
        events.push(MacroEvent::new(t, EventType::MouseButton { code: 273, state: KeyState::Press }));
        t += 50;
        events.push(MacroEvent::new(t, EventType::MouseButton { code: 273, state: KeyState::Release }));

        for _ in 0..15 {
            t += 16;
            events.push(MacroEvent::new(t, EventType::RelativeMove { dx: -15, dy: 0 }));
        }
        for _ in 0..15 {
            t += 16;
            events.push(MacroEvent::new(t, EventType::RelativeMove { dx: 0, dy: -15 }));
        }

        self.macro_data = MacroData::new("Sample Demo Pattern", events);
        self.set_notification("Loaded sample demonstration macro (Square cursor motion + Clicks)", false);
    }
}

impl Drop for MicroTaskApp {
    fn drop(&mut self) {
        self.save_settings();
    }
}

use egui::containers::panel::{CentralPanel, Panel};

impl eframe::App for MicroTaskApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.process_channels();

        // Request continuous repaint while recording or playing to keep timers and progress smooth
        if self.state == EngineState::Recording || self.state == EngineState::Playing {
            ui.ctx().request_repaint();
        }

        // Top Navigation & Status Bar Frame (Docked to top, spans 100% width)
        Panel::top("top_panel")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(18, 21, 30))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(35, 42, 58)))
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                self.render_header(ui);
            });

        // Bottom Status Bar Frame (Docked to bottom, spans 100% width)
        Panel::bottom("bottom_panel")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(15, 17, 24))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(30, 35, 48)))
                    .inner_margin(egui::Margin::symmetric(14, 8)),
            )
            .show(ui, |ui| {
                self.render_footer(ui);
            });

        // Main Central View Frame (Expands to 100% remaining width and height)
        CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 25, 36))
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| {
                // Render notification banner if active
                self.render_notification_banner(ui);

                // Stats Dashboard Row
                self.render_stats_dashboard(ui);

                ui.add_space(10.0);

                // Navigation Tabs
                self.render_tab_navigation(ui);

                ui.add_space(10.0);

                // Tab Content
                match self.current_tab {
                    AppTab::Events => self.render_events_tab(ui),
                    AppTab::Settings => self.render_settings_tab(ui),
                    AppTab::Devices => self.render_devices_tab(ui),
                    AppTab::Diagnostics => self.render_diagnostics_tab(ui),
                }
            });
    }
}

impl MicroTaskApp {
    fn render_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // App Title & Wayland Badge
            ui.label(
                RichText::new("⚡ MicroTask")
                    .size(17.0)
                    .strong()
                    .color(Color32::WHITE),
            );

            ui.label(
                RichText::new("Wayland Native")
                    .size(9.5)
                    .monospace()
                    .color(Color32::from_rgb(56, 189, 248)),
            );

            ui.add_space(8.0);

            // Engine Status Pill
            match self.state {
                EngineState::Idle => {
                    status_pill(ui, "IDLE", Color32::from_rgb(156, 163, 175), false);
                }
                EngineState::Recording => {
                    status_pill(ui, "REC (F9)", Color32::from_rgb(239, 68, 68), true);
                }
                EngineState::Playing => {
                    let loop_txt = if let Some(ref p) = self.playback_progress {
                        if let Some(tot) = p.total_repeats {
                            format!("PLAY {}/{}", p.current_repeat, tot)
                        } else {
                            format!("PLAY {}/∞", p.current_repeat)
                        }
                    } else {
                        "PLAY (F10)".to_string()
                    };
                    status_pill(ui, &loop_txt, Color32::from_rgb(52, 211, 153), true);
                }
                EngineState::Paused => {
                    status_pill(ui, "PAUSED", Color32::from_rgb(245, 158, 11), false);
                }
            }

            ui.add_space(8.0);
            if self.playback_config.anchor_on_record && self.playback_config.anchor_mode != AnchorMode::Disabled {
                let (ax, ay) = self.get_anchor_coordinates();
                ui.label(
                    RichText::new(format!("🎯 Anchor ({}, {})", ax, ay))
                        .size(11.0)
                        .color(Color32::from_rgb(251, 191, 36)),
                );
            }

            // Right-aligned Action Controls
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Emergency Stop Button
                let is_active = self.state != EngineState::Idle;
                let stop_btn = action_button(
                    ui,
                    "⏹",
                    "Panic (F11/Esc)",
                    if is_active {
                        Color32::from_rgb(185, 28, 28)
                    } else {
                        Color32::from_rgb(38, 42, 54)
                    },
                    is_active,
                );
                if stop_btn.clicked() {
                    self.emergency_panic_stop();
                }

                ui.add_space(6.0);

                // Play / Pause Button
                let (play_icon, play_label, play_bg) = match self.state {
                    EngineState::Playing => ("⏸", "Pause (F10)", Color32::from_rgb(217, 119, 6)),
                    EngineState::Paused => ("▶", "Resume (F10)", Color32::from_rgb(16, 185, 129)),
                    _ => ("▶", "Play (F10)", Color32::from_rgb(16, 185, 129)),
                };
                let play_btn = action_button(
                    ui,
                    play_icon,
                    play_label,
                    play_bg,
                    self.state == EngineState::Playing,
                );
                if play_btn.clicked() {
                    self.toggle_playback();
                }

                ui.add_space(6.0);

                // Record Button
                let (rec_icon, rec_label, rec_bg) = if self.state == EngineState::Recording {
                    ("⏹", "Stop (F9)", Color32::from_rgb(220, 38, 38))
                } else {
                    ("⏺", "Record (F9)", Color32::from_rgb(190, 24, 93))
                };
                let rec_btn = action_button(
                    ui,
                    rec_icon,
                    rec_label,
                    rec_bg,
                    self.state == EngineState::Recording,
                );
                if rec_btn.clicked() {
                    self.toggle_recording();
                }
            });
        });
    }

    fn render_notification_banner(&mut self, ui: &mut Ui) {
        let mut dismiss = false;
        if let Some((msg, is_error, timestamp)) = self.notification_banner.clone() {
            if timestamp.elapsed().as_secs() > 8 {
                self.notification_banner = None;
                return;
            }

            let bg = if is_error {
                Color32::from_rgb(88, 28, 28)
            } else {
                Color32::from_rgb(22, 60, 42)
            };
            let stroke_col = if is_error {
                Color32::from_rgb(239, 68, 68)
            } else {
                Color32::from_rgb(34, 197, 94)
            };

            egui::Frame::new()
                .fill(bg)
                .stroke(Stroke::new(1.0, stroke_col))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if is_error { "⚠" } else { "✓" }).size(14.0).color(stroke_col));
                        ui.label(RichText::new(&msg).size(12.0).color(Color32::WHITE));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕").clicked() {
                                dismiss = true;
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }

        if dismiss {
            self.notification_banner = None;
        }
    }

    fn render_stats_dashboard(&mut self, ui: &mut Ui) {
        ui.columns(4, |cols| {
            // Stat 1: Event Count
            let count_str = format!("{}", self.macro_data.event_count());
            stat_card(
                &mut cols[0],
                "RECORDED EVENTS",
                &count_str,
                "in memory buffer",
                Color32::from_rgb(56, 189, 248),
            );

            // Stat 2: Total Duration
            let duration_ms = if self.state == EngineState::Recording {
                self.recording_duration_ms
            } else {
                self.macro_data.total_duration_ms()
            };
            let secs = duration_ms as f64 / 1000.0;
            let dur_str = format!("{:.2}s", secs);
            stat_card(
                &mut cols[1],
                "TOTAL DURATION",
                &dur_str,
                if self.state == EngineState::Recording {
                    "recording live..."
                } else {
                    "nominal length"
                },
                Color32::from_rgb(168, 85, 247),
            );

            // Stat 3: Playback Speed
            let speed_str = format!("{:.2}x", self.playback_config.speed_multiplier);
            stat_card(
                &mut cols[2],
                "PLAYBACK SPEED",
                &speed_str,
                "spin_sleep clock",
                Color32::from_rgb(52, 211, 153),
            );

            // Stat 4: Hardware status
            let mice_count = self.detected_devices.iter().filter(|d| d.is_mouse).count();
            let kbd_count = self.detected_devices.iter().filter(|d| d.is_keyboard).count();
            let hw_str = format!("{}M / {}K", mice_count, kbd_count);
            stat_card(
                &mut cols[3],
                "INPUT DEVICES",
                &hw_str,
                if self.permission_status.is_all_ok() {
                    "kernel access OK"
                } else {
                    "access denied"
                },
                if self.permission_status.is_all_ok() {
                    Color32::from_rgb(34, 197, 94)
                } else {
                    Color32::from_rgb(239, 68, 68)
                },
            );
        });
    }

    fn render_tab_navigation(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let tabs = [
                (AppTab::Events, "📋 Timeline"),
                (AppTab::Settings, "⚙ Settings"),
                (AppTab::Devices, "🎮 Devices"),
                (AppTab::Diagnostics, "🛡 Diagnostics"),
            ];

            for (tab, title) in tabs {
                let is_selected = self.current_tab == tab;
                let btn = ui.selectable_label(
                    is_selected,
                    RichText::new(title)
                        .size(12.5)
                        .strong()
                        .color(if is_selected {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(156, 163, 175)
                        }),
                );
                if btn.clicked() {
                    self.current_tab = tab;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Quick Sample Macro button
                if ui
                    .button("💡 Demo")
                    .on_hover_text("Load sample demonstration pattern")
                    .clicked()
                {
                    self.load_sample_demo_macro();
                }

                ui.add_space(4.0);

                // Save Macro button
                if ui
                    .button("💾 Save")
                    .on_hover_text("Save macro to JSON file")
                    .clicked()
                {
                    self.save_macro_dialog();
                }

                ui.add_space(4.0);

                // Load Macro button
                if ui
                    .button("📂 Open")
                    .on_hover_text("Open macro from JSON file")
                    .clicked()
                {
                    self.load_macro_dialog();
                }
            });
        });
        ui.separator();
    }

    fn render_events_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Macro: '{}' ({} events)",
                    self.macro_data.metadata.name,
                    self.macro_data.event_count()
                ))
                .size(13.0)
                .strong()
                .color(Color32::WHITE),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(
                        RichText::new("🗑 Clear All")
                            .color(Color32::from_rgb(239, 68, 68)),
                    )
                    .clicked()
                {
                    self.macro_data.clear();
                    self.set_notification("Cleared macro buffer", false);
                }

                ui.add_space(6.0);

                if ui.button("✂ Normalize Timestamps").clicked() {
                    self.macro_data.normalize_timestamps();
                    self.set_notification("Normalized event timestamps", false);
                }
            });
        });

        ui.add_space(6.0);

        if self.macro_data.is_empty() {
            egui::Frame::new()
                .fill(Color32::from_rgb(26, 30, 42))
                .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 70)))
                .corner_radius(8)
                .inner_margin(egui::Margin::symmetric(20, 24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No macro events recorded yet")
                                .size(15.0)
                                .strong()
                                .color(Color32::from_rgb(200, 205, 215)),
                        );
                        ui.add_space(6.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Press F9 or click 'Record' to capture mouse movements, clicks, scrolls, and keys.\n\
                                    Or click the button below to load a sample demonstration pattern."
                                )
                                .size(11.5)
                                .color(Color32::from_rgb(140, 148, 162)),
                            )
                            .wrap(),
                        );
                        ui.add_space(10.0);
                        if ui
                            .button(
                                RichText::new("💡 Load Demo Pattern")
                                    .size(12.5)
                                    .strong()
                                    .color(Color32::from_rgb(56, 189, 248)),
                            )
                            .clicked()
                        {
                            self.load_sample_demo_macro();
                        }
                    });
                });
            return;
        }

        // Event List View
        let mut event_to_delete: Option<usize> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (idx, ev) in self.macro_data.events.iter().enumerate() {
                    let is_selected = self.selected_event_idx == Some(idx);
                    let (row_res, delete_clicked) = event_row(ui, idx, ev, is_selected);
                    if row_res.clicked() {
                        self.selected_event_idx = Some(idx);
                    }
                    if delete_clicked {
                        event_to_delete = Some(idx);
                    }
                }
            });

        if let Some(idx) = event_to_delete {
            self.macro_data.remove_event(idx);
            self.set_notification(format!("Deleted event #{}", idx + 1), false);
        }
    }

    fn render_settings_tab(&mut self, ui: &mut Ui) {
        let old_pb = self.playback_config.clone();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading("Replay & Timing Engine Configuration");
                ui.add_space(8.0);

                // Speed Multiplier
                ui.group(|ui| {
                    ui.label(
                        RichText::new("Playback Speed Multiplier")
                            .strong()
                            .color(Color32::from_rgb(56, 189, 248)),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.playback_config.speed_multiplier, 0.25..=10.0)
                                .step_by(0.05)
                                .suffix("x"),
                        );

                        // Preset buttons
                        for preset in [0.5, 1.0, 1.5, 2.0, 5.0] {
                            if ui.button(format!("{:.1}x", preset)).clicked() {
                                self.playback_config.speed_multiplier = preset;
                            }
                        }
                    });
                    ui.label(
                        RichText::new("Uses sub-millisecond spin_sleep timing for jitter-free high speed replay.")
                            .size(11.0)
                            .color(Color32::from_rgb(107, 114, 128)),
                    );
                });

                ui.add_space(10.0);

                // Loops & Repetitions
                ui.group(|ui| {
                    ui.label(
                        RichText::new("Loop Repetitions")
                            .strong()
                            .color(Color32::from_rgb(52, 211, 153)),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.playback_config.infinite_repeat, "Infinite Loop");

                        if !self.playback_config.infinite_repeat {
                            ui.add_space(16.0);
                            ui.label("Repeat Count:");
                            ui.add(egui::DragValue::new(&mut self.playback_config.repeat_count).range(1..=100_000));
                        }
                    });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Delay between loops (ms):");
                        ui.add(
                            egui::Slider::new(&mut self.playback_config.loop_delay_ms, 0..=5000)
                                .suffix(" ms"),
                        );
                    });
                });

                ui.add_space(10.0);

                // Starting Cursor Anchor (Wayland Start Position Synchronization)
                ui.group(|ui| {
                    ui.label(
                        RichText::new("🎯 Wayland Cursor Synchronization (Starting Position Anchor)")
                            .strong()
                            .color(Color32::from_rgb(251, 191, 36)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Wayland isolates windows and prevents reading global desktop coordinates. \
                            To guarantee that replay hits the exact same buttons and windows every time, \
                            MicroTask synchronizes the starting position: it places the cursor at this fixed coordinate \
                            when recording starts (F9), and at the exact same coordinate when playback starts (F10)."
                        )
                        .size(11.0)
                        .color(Color32::from_rgb(156, 163, 175)),
                    );
                    ui.add_space(8.0);

                    // Activation toggles
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.playback_config.anchor_on_record,
                            "Anchor cursor at RECORDING start (F9)",
                        );
                        ui.add_space(16.0);
                        ui.checkbox(
                            &mut self.playback_config.anchor_on_playback,
                            "Anchor cursor at PLAYBACK start (F10)",
                        );
                    });

                    ui.add_space(8.0);

                    // Mode Selection
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Anchor Mode:").strong());
                        ui.radio_value(&mut self.playback_config.anchor_mode, AnchorMode::Center, "Screen Center");
                        ui.radio_value(&mut self.playback_config.anchor_mode, AnchorMode::TopLeft, "Top-Left (0, 0)");
                        ui.radio_value(&mut self.playback_config.anchor_mode, AnchorMode::Custom, "Custom (X, Y)");
                        ui.radio_value(&mut self.playback_config.anchor_mode, AnchorMode::Disabled, "Disabled");
                    });

                    if self.playback_config.anchor_mode != AnchorMode::Disabled {
                        ui.add_space(8.0);

                        // Coordinates Inputs & Presets
                        ui.horizontal(|ui| {
                            ui.label("Target Position:");
                            if self.playback_config.anchor_mode == AnchorMode::TopLeft {
                                ui.label(RichText::new("(0, 0)").monospace().color(Color32::from_rgb(56, 189, 248)));
                            } else {
                                ui.add(egui::DragValue::new(&mut self.playback_config.anchor_x).prefix("X: ").range(0..=7680));
                                ui.add_space(8.0);
                                ui.add(egui::DragValue::new(&mut self.playback_config.anchor_y).prefix("Y: ").range(0..=4320));

                                ui.add_space(12.0);
                                if ui.button("🖥️ Detect Screen Center").clicked() {
                                    let (w, h) = detect_screen_resolution();
                                    self.playback_config.anchor_x = w / 2;
                                    self.playback_config.anchor_y = h / 2;
                                    self.playback_config.anchor_mode = AnchorMode::Center;
                                    self.set_notification(format!("Detected screen resolution {}x{}. Center set to ({}, {}).", w, h, w / 2, h / 2), false);
                                }
                                if ui.button("FHD (960, 540)").clicked() {
                                    self.playback_config.anchor_x = 960;
                                    self.playback_config.anchor_y = 540;
                                }
                                if ui.button("QHD (1280, 720)").clicked() {
                                    self.playback_config.anchor_x = 1280;
                                    self.playback_config.anchor_y = 720;
                                }
                            }
                        });

                        ui.add_space(8.0);

                        // Positioning Engine & Settling Delay
                        ui.horizontal(|ui| {
                            ui.label("Engine:");
                            egui::ComboBox::from_id_salt("anchor_method_select")
                                .selected_text(match self.playback_config.anchor_method {
                                    AnchorMethod::Auto => "Auto (ydotool / uinput fallback)",
                                    AnchorMethod::Ydotool => "ydotool daemon (ydotool mousemove -a)",
                                    AnchorMethod::NativeUinput => "Native Linux /dev/uinput corner-calibration",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.playback_config.anchor_method,
                                        AnchorMethod::Auto,
                                        "Auto (ydotool / uinput fallback)",
                                    );
                                    ui.selectable_value(
                                        &mut self.playback_config.anchor_method,
                                        AnchorMethod::Ydotool,
                                        "ydotool daemon (ydotool mousemove -a)",
                                    );
                                    ui.selectable_value(
                                        &mut self.playback_config.anchor_method,
                                        AnchorMethod::NativeUinput,
                                        "Native Linux /dev/uinput corner-calibration",
                                    );
                                });

                            ui.add_space(16.0);
                            ui.label("Settling Delay:");
                            ui.add(egui::Slider::new(&mut self.playback_config.anchor_settle_delay_ms, 0..=500).suffix(" ms"));
                        });

                        ui.add_space(8.0);

                        // Interactive Test Action
                        ui.horizontal(|ui| {
                            let ydo_status = if is_ydotool_available() {
                                "ydotool: Available"
                            } else {
                                "ydotool: Not detected (uinput fallback active)"
                            };
                            ui.label(RichText::new(ydo_status).size(10.5).color(Color32::from_rgb(107, 114, 128)));

                            ui.add_space(16.0);
                            if ui.button(
                                RichText::new("🎯 Test Move Cursor to Anchor Now")
                                    .strong()
                                    .color(Color32::from_rgb(56, 189, 248)),
                            ).clicked() {
                                let (target_x, target_y) = self.get_anchor_coordinates();
                                match position_cursor(target_x, target_y, self.playback_config.anchor_method) {
                                    Ok(()) => self.set_notification(
                                        format!("Cursor successfully moved to anchor ({}, {})!", target_x, target_y),
                                        false,
                                    ),
                                    Err(err) => self.set_notification(
                                        format!("Failed to move cursor to anchor: {}", err),
                                        true,
                                    ),
                                }
                            }
                        });
                    }
                });

                ui.add_space(10.0);

                // Global Hotkeys Info
                ui.group(|ui| {
                    ui.label(
                        RichText::new("Global Hardware Shortcuts")
                            .strong()
                            .color(Color32::from_rgb(192, 132, 252)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Global hotkeys listen directly to keyboard kernel nodes under /dev/input/event*, \
                            functioning globally even when MicroTask is completely unfocused, minimized, or behind full-screen games."
                        )
                        .size(11.0)
                        .color(Color32::from_rgb(156, 163, 175)),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Record Toggle:").strong());
                        ui.label(RichText::new("F9").monospace().color(Color32::from_rgb(56, 189, 248)));

                        ui.add_space(16.0);
                        ui.label(RichText::new("Playback Toggle:").strong());
                        ui.label(RichText::new("F10").monospace().color(Color32::from_rgb(52, 211, 153)));

                        ui.add_space(16.0);
                        ui.label(RichText::new("Emergency Panic:").strong());
                        ui.label(RichText::new("F11 or Escape").monospace().color(Color32::from_rgb(239, 68, 68)));
                    });
                });

                ui.add_space(10.0);

                // Settings Persistence & Profile Management
                ui.group(|ui| {
                    ui.label(
                        RichText::new("💾 Settings Persistence & Storage")
                            .strong()
                            .color(Color32::from_rgb(52, 211, 153)),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "All replay timing parameters, repeat counts, cursor anchor coordinates, \
                            and device filter toggles are automatically saved to your user configuration directory."
                        )
                        .size(11.0)
                        .color(Color32::from_rgb(156, 163, 175)),
                    );
                    ui.add_space(6.0);

                    let path_str = get_settings_path().display().to_string();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Storage Location:").strong());
                        ui.label(
                            RichText::new(path_str)
                                .size(11.0)
                                .monospace()
                                .color(Color32::from_rgb(56, 189, 248)),
                        );
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("💾 Save Settings Now").strong()).clicked() {
                            self.save_settings();
                            self.set_notification("Application settings successfully saved to disk!", false);
                        }

                        ui.add_space(12.0);
                        if ui.button("🔄 Reset to Factory Defaults").clicked() {
                            self.reset_to_defaults();
                        }
                    });
                });
            });

        // Automatically persist changes if any setting was modified during this frame
        if old_pb != self.playback_config {
            self.save_settings();
        }
    }

    fn render_devices_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Connected Physical Input Devices");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🔄 Refresh Devices").clicked() {
                    self.detected_devices = detect_devices();
                    self.set_notification("Refreshed input devices list", false);
                }
            });
        });

        ui.add_space(8.0);
        ui.label(
            RichText::new("Select which physical devices to capture during recording:")
                .size(12.0)
                .color(Color32::from_rgb(156, 163, 175)),
        );
        ui.add_space(6.0);

        let mut devices_changed = false;

        ScrollArea::vertical().show(ui, |ui| {
            for dev in &mut self.detected_devices {
                egui::Frame::new()
                    .fill(Color32::from_rgb(26, 30, 42))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 70)))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut dev.enabled, "").changed() {
                                devices_changed = true;
                            }

                            let icon = if dev.is_mouse { "🖱 [Mouse]" } else { "⌨ [Keyboard]" };
                            let icon_col = if dev.is_mouse {
                                Color32::from_rgb(56, 189, 248)
                            } else {
                                Color32::from_rgb(168, 85, 247)
                            };

                            ui.label(RichText::new(icon).size(12.0).strong().color(icon_col));
                            ui.label(RichText::new(&dev.name).size(13.0).strong().color(Color32::WHITE));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    RichText::new(dev.path.to_string_lossy())
                                        .size(11.0)
                                        .monospace()
                                        .color(Color32::from_rgb(107, 114, 128)),
                                );
                            });
                        });
                    });
                ui.add_space(4.0);
            }
        });

        if devices_changed {
            self.save_settings();
        }
    }

    fn render_diagnostics_tab(&mut self, ui: &mut Ui) {
        ui.heading("Kernel Permissions & System Diagnostics");
        ui.add_space(8.0);

        egui::Frame::new()
            .fill(Color32::from_rgb(26, 30, 42))
            .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 70)))
            .corner_radius(8)
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Hardware Access Checklist:")
                        .size(14.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.add_space(8.0);

                // /dev/uinput
                ui.horizontal(|ui| {
                    if self.permission_status.uinput_accessible {
                        ui.label(RichText::new("✓").strong().color(Color32::from_rgb(34, 197, 94)));
                        ui.label("/dev/uinput read & write: ACCESS GRANTED");
                    } else {
                        ui.label(RichText::new("✗").strong().color(Color32::from_rgb(239, 68, 68)));
                        ui.label(RichText::new("/dev/uinput: ACCESS DENIED").color(Color32::from_rgb(239, 68, 68)));
                    }
                });

                // /dev/input/*
                ui.horizontal(|ui| {
                    if self.permission_status.dev_input_accessible {
                        ui.label(RichText::new("✓").strong().color(Color32::from_rgb(34, 197, 94)));
                        ui.label("/dev/input/event* capture: ACCESS GRANTED");
                    } else {
                        ui.label(RichText::new("✗").strong().color(Color32::from_rgb(239, 68, 68)));
                        ui.label(RichText::new("/dev/input/event*: ACCESS DENIED").color(Color32::from_rgb(239, 68, 68)));
                    }
                });

                // input group
                ui.horizontal(|ui| {
                    if self.permission_status.in_input_group {
                        ui.label(RichText::new("✓").strong().color(Color32::from_rgb(34, 197, 94)));
                        ui.label(format!("User '{}' belongs to group 'input'", self.permission_status.user_name));
                    } else {
                        ui.label(RichText::new("⚠").strong().color(Color32::from_rgb(245, 158, 11)));
                        ui.label(format!("User '{}' not detected in 'input' group", self.permission_status.user_name));
                    }
                });
            });

        ui.add_space(14.0);

        if !self.permission_status.is_all_ok() {
            ui.group(|ui| {
                ui.label(
                    RichText::new("How to fix permissions on Linux / Wayland:")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_rgb(251, 191, 36)),
                );
                ui.add_space(6.0);
                ui.label("1. Add your user to the input group:");
                ui.label(
                    RichText::new(format!("sudo usermod -aG input {}", self.permission_status.user_name))
                        .monospace()
                        .color(Color32::from_rgb(56, 189, 248)),
                );

                ui.add_space(6.0);
                ui.label("2. Ensure uinput udev rules exist (/etc/udev/rules.d/99-uinput.rules):");
                ui.label(
                    RichText::new("KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"input\", OPTIONS+=\"static_node=uinput\"")
                        .monospace()
                        .color(Color32::from_rgb(52, 211, 153)),
                );

                ui.add_space(6.0);
                ui.label("3. Reload udev and log out & back in:");
                ui.label(
                    RichText::new("sudo udevadm control --reload-rules && sudo udevadm trigger")
                        .monospace()
                        .color(Color32::from_rgb(56, 189, 248)),
                );
            });
        } else {
            ui.group(|ui| {
                ui.label(
                    RichText::new("All hardware checks passed! No additional configuration is required.")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_rgb(34, 197, 94)),
                );
            });
        }
    }

    fn render_footer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("⚡ MicroTask v0.1.0")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(140, 148, 162)),
            );

            ui.add_space(10.0);

            if let Some(ref prog) = self.playback_progress {
                let ratio = if prog.total_events > 0 {
                    (prog.current_event_index as f32 / prog.total_events as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let rep_txt = if let Some(tot) = prog.total_repeats {
                    format!("Loop {}/{}", prog.current_repeat, tot)
                } else {
                    format!("Loop {}/∞", prog.current_repeat)
                };
                ui.label(
                    RichText::new(format!(
                        "▶ [{} | Event {}/{} | {:.1}s]",
                        rep_txt,
                        prog.current_event_index,
                        prog.total_events,
                        prog.elapsed_ms as f64 / 1000.0
                    ))
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(52, 211, 153)),
                );
                ui.add_space(6.0);
                ui.add(
                    egui::ProgressBar::new(ratio)
                        .desired_width(100.0)
                        .animate(true),
                );
            } else if self.state == EngineState::Recording {
                ui.label(
                    RichText::new(format!(
                        "● Recording... {} events ({:.1}s)",
                        self.macro_data.event_count(),
                        self.recording_duration_ms as f64 / 1000.0
                    ))
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(239, 68, 68)),
                );
            } else {
                ui.label(
                    RichText::new("Hotkeys: [F9] Record  |  [F10] Play  |  [F11 / Esc] Panic Stop")
                        .size(11.0)
                        .color(Color32::from_rgb(156, 163, 175)),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.permission_status.is_all_ok() {
                    ui.label(
                        RichText::new("✓ Kernel Access OK")
                            .size(10.5)
                            .color(Color32::from_rgb(34, 197, 94)),
                    );
                } else {
                    ui.label(
                        RichText::new("⚠ Check Permissions")
                            .size(10.5)
                            .color(Color32::from_rgb(239, 68, 68)),
                    );
                }
            });
        });
    }

    fn save_macro_dialog(&mut self) {
        if self.macro_data.is_empty() {
            self.set_notification("Cannot save an empty macro buffer", true);
            return;
        }

        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save MicroTask Macro")
            .add_filter("Macro JSON", &["json", "macro.json"])
            .set_file_name("recording.macro.json")
            .save_file()
        {
            match save_macro_to_file(&path, &self.macro_data) {
                Ok(_) => self.set_notification(format!("Saved macro to {:?}", path), false),
                Err(e) => self.set_notification(format!("Save failed: {}", e), true),
            }
        }
    }

    fn load_macro_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open MicroTask Macro")
            .add_filter("Macro JSON", &["json", "macro.json"])
            .pick_file()
        {
            match load_macro_from_file(&path) {
                Ok(data) => {
                    let mut anchor_loaded_msg = String::new();
                    if let (Some(mode), Some(x), Some(y)) = (data.metadata.anchor_mode, data.metadata.anchor_x, data.metadata.anchor_y) {
                        if mode != AnchorMode::Disabled {
                            self.playback_config.anchor_mode = mode;
                            self.playback_config.anchor_x = x;
                            self.playback_config.anchor_y = y;
                            anchor_loaded_msg = format!(" [Anchor: ({}, {})]", x, y);
                        }
                    }
                    self.macro_data = data;
                    self.set_notification(
                        format!(
                            "Loaded macro '{}' ({} events){}",
                            self.macro_data.metadata.name,
                            self.macro_data.event_count(),
                            anchor_loaded_msg
                        ),
                        false,
                    );
                }
                Err(e) => self.set_notification(format!("Load failed: {}", e), true),
            }
        }
    }
}
