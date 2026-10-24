# MicroTask

A macro recorder and player for Linux with native Wayland and X11 support.

[English](README.md) | [Русский](README.ru.md)

MicroTask records mouse and keyboard input directly from Linux kernel event interfaces (`/dev/input/event*`) and replays them through a virtual device (`/dev/uinput`). This bypasses compositor-level restrictions on modern Wayland desktop environments (KDE Plasma, GNOME, Sway, Hyprland) while remaining fully functional on X11.

## Features

- **Input capture**: Records relative mouse movements, mouse wheel, clicks (left, right, middle, extra buttons), and keyboard strokes.
- **Wayland cursor synchronization**: Anchors cursor positions at the start of recording and playback to ensure relative mouse paths hit the exact same screen targets.
- **Playback control**: Configurable speed multiplier (0.25x to 10.0x), repeat count, infinite looping, and inter-repetition delays.
- **Global shortcuts**: Background hotkey listener that works regardless of window focus or full-screen applications.
- **Emergency stop**: Panic kill-switch that halts playback and automatically releases any held keys or mouse buttons.
- **Macro editor**: View the full event timeline, inspect timestamps and deltas, remove unwanted events, and normalize timings.
- **Persistent configuration**: Application settings, timing parameters, and device filters are automatically saved to `~/.config/microtask/settings.json`.
- **JSON macro files**: Save and load macros in human-readable JSON format.

## Default Shortcuts

| Shortcut | Action |
| --- | --- |
| `F9` | Start / Stop recording |
| `F10` | Play / Pause / Resume playback |
| `Pause` or `F12` | Emergency stop (release all held keys and stop) |

## Permissions & Setup

MicroTask interacts directly with the Linux input subsystem. Your user account must have permissions to read `/dev/input/event*` and write to `/dev/uinput`.

### 1. Add user to the `input` group

```bash
sudo usermod -aG input $USER
```

### 2. Configure udev rule for `/dev/uinput`

If `/dev/uinput` is not writable by the `input` group on your distribution, create `/etc/udev/rules.d/99-uinput.rules`:

```bash
echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' | sudo tee /etc/udev/rules.d/99-uinput.rules
```

Apply the rule:

```bash
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Log out and log back in for group membership changes to take effect.

## Installation

### Arch Linux (AUR)

```bash
paru -S microtask
# or using yay
yay -S microtask
```

### Prebuilt Packages (.deb, .AppImage, tarball)

Download the latest packages from [GitHub Releases](https://github.com/Vortr3x/MicroTask/releases):
- **AppImage**: Standalone executable for any modern Linux distribution.
- **Debian / Ubuntu**: `.deb` package.
- **Standalone Archive**: `.tar.gz` with binary, desktop entry, icon, and udev rules.

## Building and Running

### Prerequisites

- Rust toolchain (stable, edition 2021 or later)
- Linux kernel with `/dev/uinput` enabled

### Development Build

```bash
cargo run
```

### Release Build

```bash
cargo build --release
./target/release/microtask
```

### Command Line Options

```bash
# Print permissions status and kernel device accessibility
./target/release/microtask --diagnostics

# List all detected physical mice and keyboards
./target/release/microtask --list-devices

# Show command line help
./target/release/microtask --help
```

## Configuration

Settings are saved automatically to `~/.config/microtask/settings.json`. The configuration file stores:
- Playback speed, repetition count, and loop delays
- Cursor anchor mode (Center, Top-Left, Custom) and coordinates
- Positioning engine selection
- Global hotkey keycodes
- Enabled and disabled input device filters

Settings can also be manually saved or reset to defaults from the Settings tab in the application.

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

