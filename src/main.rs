mod input;
mod model;
mod storage;
mod ui;

use eframe::egui;
use std::env;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use ui::MicroTaskApp;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("MicroTask - Native Wayland/Linux Macro Recorder & Player in Rust");
        println!();
        println!("USAGE:");
        println!("    microtask [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("    -h, --help           Print help information");
        println!("    -d, --diagnostics    Run hardware & permission diagnostics and print to stdout");
        println!("    -l, --list-devices   List detected mice and keyboard devices");
        println!();
        println!("GLOBAL HOTKEYS:");
        println!("    F9                   Toggle Recording (Start / Stop)");
        println!("    F10                  Toggle Playback (Play / Pause / Resume)");
        println!("    F11 or Escape        Emergency Panic Stop (Immediately releases all held keys/buttons)");
        println!();
        println!("WAYLAND CORNER ANCHOR:");
        println!("    Wayland strictly blocks synthetic absolute cursor warps.");
        println!("    Enable 'Corner Anchor' in Playback Settings to slam the cursor into (0, 0)");
        println!("    before relative playback for 100% deterministic spatial calibration.");
        return Ok(());
    }

    if args.iter().any(|a| a == "--diagnostics" || a == "-d") {
        println!("=== MicroTask Hardware & Permissions Diagnostics ===");
        let perm = input::check_permissions();
        println!("- /dev/uinput read & write: {}", if perm.uinput_accessible { "OK (Granted)" } else { "FAILED (Access Denied)" });
        println!("- /dev/input/* read access: {}", if perm.dev_input_accessible { "OK (Granted)" } else { "FAILED (Access Denied)" });
        println!("- User '{}' in input group: {}", perm.user_name, if perm.in_input_group { "YES" } else { "NO" });
        if !perm.issues.is_empty() {
            println!("\nDetected Issues:");
            for issue in &perm.issues {
                println!("  [!] {}", issue);
            }
            println!("\nFix Instructions:");
            println!("  1. sudo usermod -aG input $USER");
            println!("  2. echo 'KERNEL==\"uinput\", MODE=\"0660\", GROUP=\"input\", OPTIONS+=\"static_node=uinput\"' | sudo tee /etc/udev/rules.d/99-uinput.rules");
            println!("  3. sudo udevadm control --reload-rules && sudo udevadm trigger");
        } else {
            println!("\nAll hardware and permission checks PASSED!");
        }
        return Ok(());
    }

    if args.iter().any(|a| a == "--list-devices" || a == "-l") {
        println!("=== Detected Physical Input Devices ===");
        let devices = input::detect_devices();
        for (i, dev) in devices.iter().enumerate() {
            let kind = if dev.is_mouse && dev.is_keyboard {
                "Mouse + Keyboard"
            } else if dev.is_mouse {
                "Mouse"
            } else if dev.is_keyboard {
                "Keyboard"
            } else {
                "Other"
            };
            println!("{:2}. [{}] \"{}\" ({:?})", i + 1, kind, dev.name, dev.path);
        }
        return Ok(());
    }

    // Initialize logging with environment filter (defaulting to info)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,microtask=debug")),
        )
        .with_target(false)
        .with_thread_names(true)
        .init();

    info!("Starting MicroTask: Native Wayland/Linux Macro Recorder & Player");

    // Perform initial permission and device check
    let perm = input::check_permissions();
    if !perm.is_all_ok() {
        error!("Permission notice: {:?}", perm.issues);
    } else {
        info!("Kernel device permissions verified: /dev/uinput and /dev/input/* OK");
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MicroTask - Native Wayland/Linux Macro Recorder & Player")
            .with_inner_size([880.0, 700.0])
            .with_min_inner_size([640.0, 480.0])
            .with_app_id("com.vortr3x.microtask"),
        ..Default::default()
    };

    eframe::run_native(
        "MicroTask - Wayland Macro Recorder",
        native_options,
        Box::new(|cc| Ok(Box::new(MicroTaskApp::new(cc)))),
    )
}
