# Dimmer for Linux

Dimmer is a native GTK4 application for GNOME on Wayland. It dims each monitor
independently with a click-through overlay, keeps a master level across displays,
shows screen identities and boundaries, and remains available from the top bar
when its window is closed.

![Dimmer](data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg)

## Features

- Independent dimming for every connected monitor.
- Master control that applies one level to all displays.
- Optional screen identity numbers and monitor boundaries.
- Tint colour mixed into the dimming overlay.
- Optional adaptive dimming that adds darkness on bright content; it never
  brightens the screen or exceeds 90% total.
- Non-dimmable settings window.
- Top-bar indicator with Open, Reset window position and Quit.
- Start minimised and XDG autostart support.
- Atomic JSON settings compatible with Dimmer 2.0.1.
- Single-instance operation and a built-in Debug tab.

## Requirements

- GNOME Shell 45–51 on Wayland.
- GTK 4.12 or newer and libadwaita 1.4 or newer.
- An x86_64 Linux system for the prebuilt GitHub release.
- Rust 1.85 or newer, `pkg-config`, a C compiler and the GTK/libadwaita
  development headers for a source build.

The GNOME Shell extension is required: Wayland does not allow an ordinary
application to draw over other applications. The extension is embedded in the
Dimmer binary and installed automatically.

KDE, Xfce, other desktop environments and X11 are not supported.

## Installation

Dimmer is distributed through GitHub Releases. A source build is available as an
alternative.

### GitHub release (recommended)

```bash
curl --proto '=https' --tlsv1.2 -sSfL https://github.com/realrnvr/dimmer/releases/latest/download/install.sh | sh
```

The installer downloads the release archive, verifies its SHA-256 checksum,
and installs the application, desktop entry, icon, AppStream metadata and GNOME
Shell extension for the current user. It does not use `sudo` or modify system
files. GTK and libadwaita must already be installed.

To inspect the installer before running it:

```bash
curl -fsSL https://github.com/realrnvr/dimmer/releases/latest/download/install.sh -o install.sh
less install.sh
sh install.sh
```

Installer options are passed after `sh -s --`:

```bash
curl --proto '=https' --tlsv1.2 -sSfL \
  https://github.com/realrnvr/dimmer/releases/latest/download/install.sh \
  | sh -s -- --prefix "$HOME/.local"
```

GNOME Shell scans new extensions only at login. After the first install, log out
and back in, then enable the extension if necessary:

```bash
gnome-extensions enable dimmer@dimmer.linux
```

### Build from source

Clone the repository and run the source installer:

```bash
git clone https://github.com/realrnvr/dimmer.git
cd dimmer
./scripts/install.sh
```

The installer builds a release binary and installs it under `~/.local`. It never
invokes a system package manager and never requires root.

Install the build prerequisites with your distribution:

```bash
# Fedora
sudo dnf install cargo rust gtk4-devel libadwaita-devel pkg-config gcc \
    gnome-shell

# Debian or Ubuntu
sudo apt install cargo rustc libgtk-4-dev libadwaita-1-dev pkg-config \
    build-essential gnome-shell

# Arch Linux
sudo pacman -S --needed rust gtk4 libadwaita pkgconf gcc gnome-shell
```

`gnome-shell` provides the `gnome-extensions` command. The optional
`gnome-extensions-app` package provides the graphical Extensions application.

Source installer options:

```bash
./scripts/install.sh --help
./scripts/install.sh --prefix /tmp/test-prefix --no-enable   # staging test
./scripts/install.sh --binary target/release/dimmer          # skip the build
```

The installer writes:

| Path | Contents |
|---|---|
| `~/.local/bin/dimmer` | Application |
| `~/.local/share/applications/io.github.dimmer.Linux.desktop` | Launcher |
| `~/.local/share/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg` | Icon |
| `~/.local/share/metainfo/io.github.dimmer.Linux.metainfo.xml` | AppStream data |
| `~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux/` | Shell extension |

### Verify the installation

```bash
dimmer --version
gnome-extensions info dimmer@dimmer.linux
```

The extension should report `State: ACTIVE`. If it is not active, see
[`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md).

## Usage

```bash
dimmer                 # open the settings window
dimmer --minimised     # start hidden with dimming active
dimmer --show          # show a running instance (default)
dimmer --quit          # save and quit a running instance
dimmer --version
```

| Option | Meaning |
|---|---|
| `--show` | Show the settings window (default) |
| `--minimised`, `--minimized` | Start hidden in the background |
| `--quit` | Ask the running instance to save and exit |
| `--config <path>` | Use a specific `Dimmer.ini` |
| `--page <name>` | Open `screens`, `options`, `about` or `debug` |
| `--install-extension` | Install the bundled Shell extension and exit |
| `--debug`, `-d` | Verbose logging |
| `--version`, `-V` | Print the version |
| `--help`, `-h` | Print help |

Dimmer is single-instance. Launching it again raises the existing window; a
second `--minimised` launch exits without raising it.

### Screens

- **Master** appears when more than one monitor is connected. Its slider applies
  the same dim level to every screen. Turning master off releases the lock but
  keeps the last per-screen values.
- **Monitor cards** enable or disable a monitor, set an exact dim level from 0 to
  90%, and choose a tint colour.
- **Show screen identity** draws a large number at the top-left of each monitor.
- **Show screen boundaries** draws a yellow border around each monitor.
- **Non-dimmable window** cuts the dim layer around the settings window so it
  stays readable while the rest of its monitor is dimmed.

### Adaptive dimming

Optional and off by default. When enabled, Dimmer measures the brightness of the
dimmed screen four times a second and adds just enough extra black to bring
bright content under two calibrated ceilings. It can only darken: the layer is
capped at 50% extra and at 90% total, so it never brightens the screen and never
undoes your chosen dim.

- The measurement runs inside GNOME Shell and is discarded after each sample.
  Nothing is recorded, saved or sent anywhere.
- The panel and the non-dimmable settings window are excluded, because Dimmer
  does not dim them.
- The response is deterministic. Strength 0–100 moves the peak/average targets
  from 72%/42% to 52%/28% brightness, and the smallest extra darkness that meets
  both is applied. Darkness is added at up to 0.8 alpha/s and released at 0.1
  alpha/s, so a passing bright window does not flash.
- Each monitor is measured and controlled independently. Adaptive is inactive at
  0% screen dim (there is no dim layer to build on) and at 90% (no headroom
  left).
- It pauses behind the overview, the lock screen, Shell dialogs and open menus,
  and resumes when they close.

The table is the response for uniformly bright content with the default strength
(50) and the screen dim at 10%, rounded to 1%. `A` is the fraction of the
measured (dimmed) area that is white. At 0% screen dim adaptive is inactive, and
higher dim levels leave less headroom and react later.

| White area `A` | Extra darkness |
|---|---|
| 0–14% | 0 |
| 15% | 9% |
| 20–56% | 32% |
| 60% | 36% |
| 70% | 45% |
| 77–100% | 50% (cap) |

"Deterministic" describes the software response, not a promise of comfort:
ambient light, monitor gamma and individual sensitivity are outside what the
extension can measure. Use Strength to taste. To see the response at 10% dim,
open `gnome-extension/tests/adaptive-flash.html` fullscreen and watch
`gjs -m gnome-extension/tests/adaptive-watch.js`; see
[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md).

### Options

- Start minimised.
- Auto run through an XDG autostart entry.
- Movable window.
- Close hides the window and keeps Dimmer running in the top bar.
- Show in taskbar is stored for settings compatibility; GTK4/Wayland cannot
  change taskbar visibility.

### Top-bar indicator

The panel menu provides Start/Open Dimmer, Reset window position and Quit. Master
dimming remains in the application window.

## Configuration

Settings are stored as JSON in `Dimmer.ini`. The first available location is
used:

1. `--config <path>`
2. `Dimmer.ini` next to the executable, if present and writable
3. `$XDG_CONFIG_HOME/dimmer/Dimmer.ini`
4. `~/.config/dimmer/Dimmer.ini`

```json
{
  "ShowIds": false,
  "ShowBoundaries": false,
  "AutoRun": { "On": false, "AllUsers": false, "Mode": "StartupFolder" },
  "MovableWindow": true,
  "StartMinimised": false,
  "NonDimmableWindow": false,
  "CloseHidesToTray": false,
  "ShowInTaskbar": true,
  "Master": { "On": false, "Dim": 30.0 },
  "Adaptive": { "On": false, "Strength": 50.0 },
  "Screens": [
    { "Id": 1, "On": true, "Dim": 30.0, "Tint": "000000" }
  ],
  "LastRun": "2026-10-26 01:00:00"
}
```

Writes are atomic. A malformed file is backed up as
`Dimmer.ini.bak-<timestamp>` and replaced with defaults; the error is shown in
the Debug tab. Unknown fields are ignored, so settings files from the Windows
version can be copied across.

## Uninstall

```bash
./scripts/uninstall.sh           # keep settings
./scripts/uninstall.sh --purge   # also remove ~/.config/dimmer
```

The script stops a running instance, disables the extension, and removes the
binary, launcher, icon, AppStream metadata, extension directory and autostart
entry. It does not require root.

## Troubleshooting

See [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) for extension-load,
dimming, settings, autostart and packaging problems.

## Development

Build and test the Rust workspace:

```bash
cargo build --release
cargo test --locked --workspace
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
```

If libadwaita is installed in a non-standard prefix, point `pkg-config` at it:

```bash
PKG_CONFIG_PATH=/path/to/lib/pkgconfig cargo build --release
```

Run the extension tests:

```bash
gjs -m gnome-extension/tests/metadata.test.js
gjs -m gnome-extension/tests/overlay-math.test.js
gjs -m gnome-extension/tests/window-reset.test.js
```

Run the private GNOME Shell harnesses (they do not touch the live session):

```bash
DIMMER_BIN=$PWD/target/release/dimmer timeout 240 \
  dbus-run-session -- bash gnome-extension/tests/headless-shell.sh

DIMMER_BIN=$PWD/target/release/dimmer timeout 180 \
  dbus-run-session -- bash gnome-extension/tests/multimon-shell.sh
```

Set `EXTENSION_BUNDLE=/path/to/dimmer@dimmer.linux.shell-extension.zip` to test
the packed artifact instead of the source directory.

Run the headless GTK suites:

```bash
DIMMER_BIN=$PWD/target/release/dimmer ./gui-tests/run.sh
```

The GUI suites cover keyboard focus, page traversal, slider and entry
synchronisation, autostart toggles and the Donate button. They need GStreamer
development packages and run in a private D-Bus session. See
[`gui-tests/README.md`](gui-tests/README.md).

Package the GNOME extension:

```bash
./scripts/pack-extension.sh
```

This writes `dist/dimmer@dimmer.linux.shell-extension.zip` and verifies that the
archive contains all eight runtime files.

## Project layout

```text
dimmer/
├── crates/dimmer-core/       pure model, settings IO, dim maths, autostart
├── crates/dimmer/            GTK4 UI, D-Bus service, lifecycle, CLI
├── gnome-extension/
│   ├── dimmer@dimmer.linux/  GNOME Shell extension
│   └── tests/                gjs, headless Shell and two-monitor harnesses
├── gui-tests/                WayDriver GUI suites (standalone workspace)
├── data/                     desktop entry, icon, AppStream metadata
├── docs/                     architecture and troubleshooting
└── scripts/                  install, uninstall, packaging and tests
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for design details.

## Credits and license

Dimmer for Linux is an independent reimplementation inspired by **Dimmer 2.0.1**
by Nelson Pires (<https://www.nelsonpires.com/software/dimmer>). The original
application and its assets remain the property of their author. The Linux port
does not include the original application's source or assets.

This port is released under the MIT License. See [`LICENSE`](LICENSE).
