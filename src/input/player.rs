use crate::input::cursor_anchor::{position_cursor, resolve_anchor_coordinates};
use crate::input::uinput_emitter::UinputEmitter;
use crate::model::config::{AnchorMode, PlaybackConfig};
use crate::model::event::MacroEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

#[derive(Clone, Debug)]
pub struct PlaybackProgress {
    pub current_repeat: u32,
    pub total_repeats: Option<u32>, // None if infinite
    pub current_event_index: usize,
    pub total_events: usize,
    pub elapsed_ms: u64,
    pub is_finished: bool,
}

#[allow(dead_code)]
pub struct PlayerHandle {
    stop_flag: Arc<AtomicBool>,
    pause_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

#[allow(dead_code)]
impl PlayerHandle {
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.pause_flag.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.pause_flag.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.pause_flag.load(Ordering::SeqCst)
    }

    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::SeqCst)
    }

    pub fn join(mut self) {
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct MacroPlayer;

impl MacroPlayer {
    pub fn play(
        events: Vec<MacroEvent>,
        config: PlaybackConfig,
        progress_sender: Option<crossbeam_channel::Sender<PlaybackProgress>>,
    ) -> Result<PlayerHandle, String> {
        if events.is_empty() {
            return Err("Cannot play an empty macro".to_string());
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let pause_flag = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop_flag);
        let thread_pause = Arc::clone(&pause_flag);

        let join_handle = thread::Builder::new()
            .name("microtask-player".to_string())
            .spawn(move || {
                run_playback_engine(events, config, thread_stop, thread_pause, progress_sender);
            })
            .map_err(|e| format!("Failed to spawn playback thread: {}", e))?;

        Ok(PlayerHandle {
            stop_flag,
            pause_flag,
            join_handle: Some(join_handle),
        })
    }
}

fn run_playback_engine(
    events: Vec<MacroEvent>,
    config: PlaybackConfig,
    stop_flag: Arc<AtomicBool>,
    pause_flag: Arc<AtomicBool>,
    progress_sender: Option<crossbeam_channel::Sender<PlaybackProgress>>,
) {
    let mut emitter = match UinputEmitter::new() {
        Ok(e) => e,
        Err(err) => {
            error!("Playback failed to initialize virtual device: {}", err);
            send_progress(
                &progress_sender,
                PlaybackProgress {
                    current_repeat: 0,
                    total_repeats: None,
                    current_event_index: 0,
                    total_events: events.len(),
                    elapsed_ms: 0,
                    is_finished: true,
                },
            );
            return;
        }
    };

    // Synchronize starting cursor position before replaying events
    if config.anchor_on_playback && config.anchor_mode != AnchorMode::Disabled {
        let (target_x, target_y) = resolve_anchor_coordinates(
            config.anchor_mode,
            config.anchor_x,
            config.anchor_y,
        );
        info!(
            "Playback cursor anchor: positioning to ({}, {}) via {:?}",
            target_x, target_y, config.anchor_method
        );
        if let Err(e) = position_cursor(target_x, target_y, config.anchor_method) {
            warn!("Playback starting anchor positioning error: {}", e);
        }
        if config.anchor_settle_delay_ms > 0 {
            sleep(Duration::from_millis(config.anchor_settle_delay_ms));
        }
    } else if config.corner_anchor {
        // Legacy fallback
        info!("Wayland corner anchor enabled: resetting cursor to (0, 0)");
        if let Err(e) = emitter.anchor_cursor_to_top_left(
            config.anchor_burst_count,
            config.anchor_burst_step,
            config.anchor_burst_delay_ms,
        ) {
            warn!("Corner anchor error: {}", e);
        }
    }

    let speed = (config.speed_multiplier as f64).clamp(0.1, 50.0);
    let total_events = events.len();
    let total_repeats = if config.infinite_repeat {
        None
    } else {
        Some(config.repeat_count.max(1))
    };

    info!(
        "Starting macro playback (events: {}, speed: {:.2}x, repeats: {:?})",
        total_events, speed, total_repeats
    );

    let mut current_repeat: u32 = 0;

    'repeat_loop: loop {
        current_repeat += 1;
        if let Some(max_rep) = total_repeats {
            if current_repeat > max_rep {
                break 'repeat_loop;
            }
        }

        debug!("Beginning repeat loop {}/{:?}", current_repeat, total_repeats);

        let repeat_start = Instant::now();
        let mut prev_timestamp_ms: u64 = events[0].timestamp_ms;

        for (idx, macro_event) in events.iter().enumerate() {
            // Check stop flag
            if stop_flag.load(Ordering::Relaxed) {
                warn!("Playback received emergency stop signal!");
                break 'repeat_loop;
            }

            // Check pause flag
            while pause_flag.load(Ordering::Relaxed) {
                if stop_flag.load(Ordering::Relaxed) {
                    break 'repeat_loop;
                }
                sleep(Duration::from_millis(15));
            }

            // Calculate precise delta sleep
            if idx > 0 {
                let delta_ms = macro_event.timestamp_ms.saturating_sub(prev_timestamp_ms);
                let sleep_secs = (delta_ms as f64 / 1000.0) / speed;

                if sleep_secs > 0.0001 {
                    responsive_sleep(
                        Duration::from_secs_f64(sleep_secs),
                        &stop_flag,
                        &pause_flag,
                    );
                }
            }
            prev_timestamp_ms = macro_event.timestamp_ms;

            // Emit event
            if let Err(e) = emitter.emit(&macro_event.event) {
                error!("Error emitting macro event: {}", e);
            }

            // Send progress update
            let elapsed_ms = repeat_start.elapsed().as_millis() as u64;
            send_progress(
                &progress_sender,
                PlaybackProgress {
                    current_repeat,
                    total_repeats,
                    current_event_index: idx + 1,
                    total_events,
                    elapsed_ms,
                    is_finished: false,
                },
            );
        }

        // Delay between repetitions
        if config.loop_delay_ms > 0 {
            let delay = Duration::from_millis(config.loop_delay_ms);
            responsive_sleep(delay, &stop_flag, &pause_flag);
        }

        if stop_flag.load(Ordering::Relaxed) {
            break 'repeat_loop;
        }
    }

    // Emergency release any keys or buttons held down
    emitter.release_all_held();

    info!("Macro playback finished cleanly.");
    send_progress(
        &progress_sender,
        PlaybackProgress {
            current_repeat,
            total_repeats,
            current_event_index: total_events,
            total_events,
            elapsed_ms: 0,
            is_finished: true,
        },
    );
}

/// Sleeps with sub-millisecond spin_sleep precision while checking stop and pause flags
fn responsive_sleep(
    duration: Duration,
    stop_flag: &Arc<AtomicBool>,
    pause_flag: &Arc<AtomicBool>,
) {
    let start = Instant::now();
    let slice = Duration::from_millis(10);

    while start.elapsed() < duration {
        if stop_flag.load(Ordering::Relaxed) || pause_flag.load(Ordering::Relaxed) {
            break;
        }

        let remaining = duration.saturating_sub(start.elapsed());
        if remaining > slice {
            spin_sleep::sleep(slice);
        } else {
            spin_sleep::sleep(remaining);
            break;
        }
    }
}

#[inline]
fn send_progress(
    sender: &Option<crossbeam_channel::Sender<PlaybackProgress>>,
    prog: PlaybackProgress,
) {
    if let Some(s) = sender {
        let _ = s.try_send(prog);
    }
}
