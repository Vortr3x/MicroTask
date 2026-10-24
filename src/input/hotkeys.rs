use crate::input::device_detector::detect_devices;
use crate::model::config::HotkeyConfig;
use evdev::Device;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tracing::{debug, error, info, warn};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    RecordToggle,
    PlayToggle,
    PanicStop,
}

pub struct HotkeyDaemonHandle {
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl HotkeyDaemonHandle {
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(h) = self.join_handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for HotkeyDaemonHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct HotkeyDaemon;

impl HotkeyDaemon {
    pub fn start(
        config: HotkeyConfig,
        sender: crossbeam_channel::Sender<HotkeyAction>,
    ) -> Result<HotkeyDaemonHandle, String> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop_flag);

        // Find all keyboard devices
        let keyboard_paths: Vec<PathBuf> = detect_devices()
            .into_iter()
            .filter(|d| d.is_keyboard)
            .map(|d| d.path)
            .collect();

        if keyboard_paths.is_empty() {
            return Err("No keyboard devices detected for global hotkey daemon".to_string());
        }

        let join_handle = thread::Builder::new()
            .name("microtask-hotkeys".to_string())
            .spawn(move || {
                run_hotkey_loop(keyboard_paths, config, thread_stop, sender);
            })
            .map_err(|e| format!("Failed to spawn hotkey daemon thread: {}", e))?;

        Ok(HotkeyDaemonHandle {
            stop_flag,
            join_handle: Some(join_handle),
        })
    }
}

fn run_hotkey_loop(
    paths: Vec<PathBuf>,
    config: HotkeyConfig,
    stop_flag: Arc<AtomicBool>,
    sender: crossbeam_channel::Sender<HotkeyAction>,
) {
    let mut devices = Vec::new();

    for path in &paths {
        match Device::open(path) {
            Ok(dev) => {
                let _ = dev.set_nonblocking(true);
                debug!("Hotkey daemon monitoring keyboard {:?}: {}", path, dev.name().unwrap_or(""));
                devices.push(dev);
            }
            Err(e) => {
                warn!("Hotkey daemon cannot open keyboard {:?}: {}", path, e);
            }
        }
    }

    if devices.is_empty() {
        error!("Hotkey daemon has no accessible keyboards");
        return;
    }

    info!(
        "Global Hotkey Daemon running across {} keyboards (F9: Record, F10: Play, Pause/F12: Panic)",
        devices.len()
    );

    let poll_timeout_ms = 100;

    while !stop_flag.load(Ordering::Relaxed) {
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
            break;
        }

        if ret == 0 {
            continue;
        }

        for (i, pfd) in fds.iter().enumerate() {
            if pfd.revents & libc::POLLIN != 0 {
                let device = &mut devices[i];
                if let Ok(events) = device.fetch_events() {
                    for ev in events {
                        if ev.event_type() == evdev::EventType::KEY && ev.value() == 1 {
                            // Key Press
                            let code = ev.code();

                            // Check configured hotkeys
                            if code == config.record_toggle_code {
                                info!("Global hotkey: RECORD TOGGLE ({})", code);
                                let _ = sender.send(HotkeyAction::RecordToggle);
                            } else if code == config.play_toggle_code {
                                info!("Global hotkey: PLAY TOGGLE ({})", code);
                                let _ = sender.send(HotkeyAction::PlayToggle);
                            } else if code == config.panic_abort_code
                                || (config.panic_abort_code == 119 && code == 88)
                                || (config.panic_abort_code == 88 && code == 119)
                            {
                                info!("Global hotkey: PANIC STOP ({})", code);
                                let _ = sender.send(HotkeyAction::PanicStop);
                            }
                        }
                    }
                }
            }
        }
    }

    info!("Hotkey daemon stopped");
}
