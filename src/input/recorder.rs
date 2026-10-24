use crate::input::device_detector::detect_devices;
use crate::model::event::{EventType, KeyState, MacroEvent};
use evdev::{Device, RelativeAxisCode};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tracing::{debug, error, info, warn};

pub struct RecorderHandle {
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<Vec<MacroEvent>>>,
}

impl RecorderHandle {
    pub fn stop(mut self) -> Vec<MacroEvent> {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join_handle.take() {
            handle.join().unwrap_or_default()
        } else {
            Vec::new()
        }
    }

    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::SeqCst)
    }
}

pub struct InputRecorder;

impl InputRecorder {
    /// Start recording events from all enabled devices in a background thread
    pub fn start(
        device_paths: Option<Vec<PathBuf>>,
        event_sender: Option<crossbeam_channel::Sender<MacroEvent>>,
        stop_key_code: Option<u16>,
    ) -> Result<RecorderHandle, String> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop_flag = Arc::clone(&stop_flag);

        // Determine which devices to open
        let paths = match device_paths {
            Some(p) => p,
            None => detect_devices()
                .into_iter()
                .filter(|d| d.enabled)
                .map(|d| d.path)
                .collect(),
        };

        if paths.is_empty() {
            return Err("No input devices selected or detected for recording".to_string());
        }

        let join_handle = thread::Builder::new()
            .name("microtask-recorder".to_string())
            .spawn(move || {
                run_recorder_loop(paths, thread_stop_flag, event_sender, stop_key_code)
            })
            .map_err(|e| format!("Failed to spawn recording thread: {}", e))?;

        Ok(RecorderHandle {
            stop_flag,
            join_handle: Some(join_handle),
        })
    }
}

fn run_recorder_loop(
    paths: Vec<PathBuf>,
    stop_flag: Arc<AtomicBool>,
    event_sender: Option<crossbeam_channel::Sender<MacroEvent>>,
    stop_key_code: Option<u16>,
) -> Vec<MacroEvent> {
    let mut devices = Vec::new();

    for path in &paths {
        match Device::open(path) {
            Ok(dev) => {
                let _ = dev.set_nonblocking(true);
                debug!("Recorder opened device {:?}: {}", path, dev.name().unwrap_or(""));
                devices.push(dev);
            }
            Err(e) => {
                warn!("Recorder failed to open {:?}: {}", path, e);
            }
        }
    }

    if devices.is_empty() {
        error!("No devices could be opened for recording");
        return Vec::new();
    }

    info!("Recording started across {} devices", devices.len());
    let mut recorded_events = Vec::new();
    let start_time = Instant::now();

    // Accumulators for relative mouse motion between SYN_REPORTs
    let mut accum_dx: i32 = 0;
    let mut accum_dy: i32 = 0;

    let poll_timeout_ms = 50;

    while !stop_flag.load(Ordering::Relaxed) {
        // Prepare pollfd array
        let mut fds: Vec<libc::pollfd> = devices
            .iter()
            .map(|d| libc::pollfd {
                fd: d.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();

        let ret = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as libc::nfds_t,
                poll_timeout_ms,
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            error!("libc::poll error in recorder: {}", err);
            break;
        }

        if ret == 0 {
            // Timeout, loop to check stop_flag
            continue;
        }

        let current_time_ms = start_time.elapsed().as_millis() as u64;

        for (i, pfd) in fds.iter().enumerate() {
            if pfd.revents & libc::POLLIN != 0 {
                let device = &mut devices[i];
                if let Ok(events) = device.fetch_events() {
                    for ev in events {
                        let ev_type = ev.event_type();
                        let ev_code = ev.code();
                        let ev_val = ev.value();

                        match ev_type {
                            evdev::EventType::RELATIVE => {
                                if ev_code == RelativeAxisCode::REL_X.0 {
                                    accum_dx += ev_val;
                                } else if ev_code == RelativeAxisCode::REL_Y.0 {
                                    accum_dy += ev_val;
                                } else if ev_code == RelativeAxisCode::REL_WHEEL.0 {
                                    // Flush any pending movement first
                                    flush_relative_move(
                                        &mut accum_dx,
                                        &mut accum_dy,
                                        current_time_ms,
                                        &mut recorded_events,
                                        &event_sender,
                                    );
                                    let macro_ev = MacroEvent::new(
                                        current_time_ms,
                                        EventType::MouseWheel {
                                            delta: ev_val,
                                            horizontal: false,
                                        },
                                    );
                                    record_one(macro_ev, &mut recorded_events, &event_sender);
                                } else if ev_code == RelativeAxisCode::REL_HWHEEL.0 {
                                    flush_relative_move(
                                        &mut accum_dx,
                                        &mut accum_dy,
                                        current_time_ms,
                                        &mut recorded_events,
                                        &event_sender,
                                    );
                                    let macro_ev = MacroEvent::new(
                                        current_time_ms,
                                        EventType::MouseWheel {
                                            delta: ev_val,
                                            horizontal: true,
                                        },
                                    );
                                    record_one(macro_ev, &mut recorded_events, &event_sender);
                                }
                            }
                            evdev::EventType::KEY => {
                                // Ignore auto-repeat key events (val == 2)
                                if ev_val == 0 || ev_val == 1 {
                                    let state = if ev_val == 1 {
                                        KeyState::Press
                                    } else {
                                        KeyState::Release
                                    };

                                    // If this is the stop hotkey, break out immediately
                                    if let Some(stop_code) = stop_key_code {
                                        if ev_code == stop_code {
                                            info!("Stop recording key ({}) pressed, stopping", ev_code);
                                            stop_flag.store(true, Ordering::SeqCst);
                                            break;
                                        }
                                    }

                                    // Flush pending move
                                    flush_relative_move(
                                        &mut accum_dx,
                                        &mut accum_dy,
                                        current_time_ms,
                                        &mut recorded_events,
                                        &event_sender,
                                    );

                                    let event_type = if (0x110..=0x11F).contains(&ev_code) {
                                        EventType::MouseButton {
                                            code: ev_code,
                                            state,
                                        }
                                    } else {
                                        EventType::KeyboardKey {
                                            code: ev_code,
                                            state,
                                        }
                                    };

                                    let macro_ev = MacroEvent::new(current_time_ms, event_type);
                                    record_one(macro_ev, &mut recorded_events, &event_sender);
                                }
                            }
                            evdev::EventType::SYNCHRONIZATION => {
                                // Flush movement on SYN_REPORT
                                flush_relative_move(
                                    &mut accum_dx,
                                    &mut accum_dy,
                                    current_time_ms,
                                    &mut recorded_events,
                                    &event_sender,
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Flush any remaining move
    let final_time_ms = start_time.elapsed().as_millis() as u64;
    flush_relative_move(
        &mut accum_dx,
        &mut accum_dy,
        final_time_ms,
        &mut recorded_events,
        &event_sender,
    );

    info!("Recording stopped. Captured {} total events", recorded_events.len());
    recorded_events
}

#[inline]
fn flush_relative_move(
    accum_dx: &mut i32,
    accum_dy: &mut i32,
    timestamp_ms: u64,
    events: &mut Vec<MacroEvent>,
    sender: &Option<crossbeam_channel::Sender<MacroEvent>>,
) {
    if *accum_dx != 0 || *accum_dy != 0 {
        let ev = MacroEvent::new(
            timestamp_ms,
            EventType::RelativeMove {
                dx: *accum_dx,
                dy: *accum_dy,
            },
        );
        *accum_dx = 0;
        *accum_dy = 0;
        record_one(ev, events, sender);
    }
}

#[inline]
fn record_one(
    ev: MacroEvent,
    events: &mut Vec<MacroEvent>,
    sender: &Option<crossbeam_channel::Sender<MacroEvent>>,
) {
    if let Some(s) = sender {
        let _ = s.try_send(ev.clone());
    }
    events.push(ev);
}
