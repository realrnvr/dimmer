# Dimmer for Linux

A Rust recreation of the Windows app **Dimmer 2.0.1** for GNOME on Wayland.
It dims each monitor independently with a click-through overlay, keeps a master
level across displays, shows screen IDs and boundaries, and lives in the top bar
when the window is closed.

![Dimmer](data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg)

## How it works

Wayland does not let an ordinary application draw over, position itself over, or
observe other applications. On GNOME the supported way to dim everything is a
GNOME Shell extension that renders an actor in the compositor.

Dimmer therefore has two parts:

| Part | Language | Role |
|---|---|---|
| `dimmer` | Rust + GTK4/libadwaita | Settings UI, state, validation, persistence, D-Bus service, diagnostics |
| `dimmer@dimmer.linux` | GJS (GNOME Shell 45–50) | Click-through dim overlays, screen IDs, boundaries, top-bar indicator |

The Rust app owns all settings and logic. The extension is a renderer and remote
control: it receives state over D-Bus and sends slider/menu changes back. If the
app exits, the extension removes its overlays automatically.

## Supported systems

Dimmer targets **GNOME on Wayland** with GNOME Shell 45–50. The compositor-side
GNOME extension is required to dim other windows, so KDE, Xfce, other desktop
environments, and X11 are not supported by this release.

The first Flatpak release targets x86_64 and supplies GTK and libadwaita
consistently across distributions. The GNOME Shell extension is installed
separately on the host. Other architectures can try a source build but are not
release targets yet.
The source build needs Rust 1.85 or newer, GTK 4.12+, libadwaita 1.4+ development
headers, `pkg-config`, and the GNOME extension tools.
Some distributions package an older Rust compiler; check `rustc --version` and
use a current Rust toolchain if it is below 1.85.

## Install

### Recommended: Flatpak

This install path becomes available once the Flathub submission is accepted.
From the repository root:

```bash
./scripts/install-flatpak.sh
```

The script adds Flathub as a user remote if needed, installs or updates the app
for the current user, downloads and verifies the matching host extension release
asset, and installs it for the current user. It does not use `sudo` or install
system packages. Log out and back in after installing the extension, then enable
it if needed:

```bash
gnome-extensions enable dimmer@dimmer.linux
```

Launch from the app grid or run:

```bash
flatpak run io.github.dimmer.Linux
```

Flatpak keeps settings in its application-specific config directory. Autostart
through Dimmer's **Auto run** switch is unavailable in the Flatpak build; use the
desktop's startup application settings instead.

### Build and install from source

Install the build prerequisites using your distribution's package manager, then
from the repository root:

```bash
./scripts/install.sh
```

Example prerequisite commands:

```bash
# Fedora
sudo dnf install cargo rust gtk4-devel libadwaita-devel pkg-config gcc \
    gnome-shell gnome-extensions

# Debian or Ubuntu
sudo apt install cargo rustc libgtk-4-dev libadwaita-1-dev pkg-config build-essential \
    gnome-shell gnome-shell-extensions

# Arch Linux
sudo pacman -S --needed rust gtk4 libadwaita pkgconf gcc gnome-shell gnome-shell-extensions
```

The source installer builds a release binary and installs it under `~/.local`.
It relies on dependencies already installed by the user; it never invokes a
system package manager.

The installer builds a release binary, then installs, for the current user only:

| Path | Contents |
|---|---|
| `~/.local/bin/dimmer` | application |
| `~/.local/share/applications/io.github.dimmer.Linux.desktop` | launcher |
| `~/.local/share/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg` | icon |
| `~/.local/share/metainfo/io.github.dimmer.Linux.metainfo.xml` | AppStream data |
| `~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux/` | Shell extension |

No root access and no system files are needed.

**GNOME Shell only scans new extensions at login.** Log out and back in after the
first install, then enable it if necessary:

```bash
gnome-extensions enable dimmer@dimmer.linux
```

Source installer options:

```bash
./scripts/install.sh --help
./scripts/install.sh --prefix /tmp/test-prefix --no-enable   # staging test
./scripts/install.sh --binary target/release/dimmer          # skip the build
```

For a local Flatpak development build, see [`flatpak/README.md`](flatpak/README.md).

To install the embedded extension on its own from a native binary:

```bash
dimmer --install-extension
```

## Usage

```bash
dimmer                 # open the settings window
dimmer --minimised     # start hidden, dimming active
dimmer --show          # show a running instance (also the default)
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
| `--install-extension` | Install the bundled Shell extension and exit (native installs) |
| `--debug`, `-d` | Verbose logging |
| `--version`, `-V` / `--help`, `-h` | Version / help |

Dimmer is single-instance. Launching it again raises the existing window; a second
`--minimised` launch exits without raising it.

### Screens tab

- **Master** — appears when more than one monitor is connected. Its slider applies
  the same dim to every screen. Turning master off releases the lock but keeps the
  last applied per-screen values; move an individual slider to diverge.
- **Each monitor card** — enable/disable the monitor, drag the slider or use the
  mouse wheel, type an exact value in the spin button (0–90%), and pick a tint
  colour. The original stored tints but did not apply them; this port applies the
  selected colour to the overlay.
- **Show screen identity** — draws a large number at the top-left of each monitor.
- **Show screen boundaries** — draws a 5 px yellow border around each monitor.
- **Non-dimmable window** — when on, the dim layer is cut around the settings
  window so the window stays readable while the rest of its monitor is dimmed.

### Options tab

- Start minimised
- Auto run (XDG autostart entry generated from the current settings)
- Movable window (drag by the header bar)
- Close hides to tray (the top-bar indicator is the tray)
- Show in taskbar (stored for compatibility; see limitations)

### About / Debug

About shows version, license, credits and links. Debug shows the OS, session type,
monitor list, extension status, settings path, settings JSON and the log, with a
copy button.

### Top-bar indicator

The panel menu offers Start/Open Dimmer, Reset window position and Quit. Master
dimming stays in the application window; the panel does not duplicate it.

## Settings

Settings are stored as JSON in a file named `Dimmer.ini`, using the original field
names:

1. `--config <path>`
2. `Dimmer.ini` next to the executable, if present and writable (portable mode)
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
  "Screens": [
    { "Id": 1, "On": true, "Dim": 30.0, "Tint": "000000" }
  ],
  "LastRun": "2026-10-26 01:00:00"
}
```

Writes are atomic. A malformed file is backed up as
`Dimmer.ini.bak-<timestamp>` and replaced with defaults; the error is shown in
Debug. Unknown fields are ignored, so files from the Windows version can be
copied across.

## Build and test

```bash
cargo build --release
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

If libadwaita is installed in a non-standard prefix, point `pkg-config` at it:

```bash
PKG_CONFIG_PATH=/path/to/lib/pkgconfig cargo build --release
```

Extension tests:

```bash
# Pure geometry/colour maths
gjs -m gnome-extension/tests/overlay-math.test.js

# Full private GNOME Shell (headless; does not touch the real session)
DIMMER_BIN=$PWD/target/release/dimmer timeout 240 \
  dbus-run-session -- bash gnome-extension/tests/headless-shell.sh

# Two virtual monitors: IDs, boundaries, per-screen and master dimming
DIMMER_BIN=$PWD/target/release/dimmer timeout 180 \
  dbus-run-session -- bash gnome-extension/tests/multimon-shell.sh
```

The headless harness starts its own session bus, `XDG_*` trees and a
`gnome-shell --headless --unsafe-mode` instance, so it is safe to run on a live
desktop. It also installs a private desktop entry with an absolute `Exec` and an
isolated `HOME`, checks that the panel is launcher/status only, then checks that
the indicator's Start Dimmer path launches the app. Set
`EXTENSION_BUNDLE=/path/to/dimmer@dimmer.linux.shell-extension.zip` to test the
packed artifact instead of the source directory.

GUI tests (headless Wayland + AT-SPI through WayDriver):

```bash
DIMMER_BIN=$PWD/target/release/dimmer ./gui-tests/run.sh
```

The suites cover header tab/close/minimise clicks, page traversal and keyboard
focus, the dim slider/entry/step-button sync, autostart toggles and the Donate
button. They need GStreamer development packages and run in a private D-Bus
session, so they are a separate workspace from the main `cargo test` gate. See
[`gui-tests/README.md`](gui-tests/README.md).

### Package the extension

```bash
./scripts/pack-extension.sh            # writes dist/dimmer@dimmer.linux.shell-extension.zip
```

`gnome-extensions pack` omits ES modules unless each one is passed with
`--extra-source`; the script does that and verifies the zip contains all seven
files.

## Uninstall

For Flatpak installations:

```bash
flatpak uninstall --user io.github.dimmer.Linux
gnome-extensions disable dimmer@dimmer.linux
rm -rf ~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux
```

The Flatpak settings are kept unless removed separately from
`~/.var/app/io.github.dimmer.Linux/`.

For source installations:

```bash
./scripts/uninstall.sh           # keep settings
./scripts/uninstall.sh --purge   # also remove ~/.config/dimmer
```

The script stops a running instance, disables the extension, and removes the
binary, launcher, icon, metainfo, extension directory and autostart entry.

## Limitations and deliberate differences

- **The extension is required.** Wayland offers no alternative for dimming other
  applications. The app remains usable without it and shows an install banner.
- **The top bar stays bright.** Overlays sit above windows but below GNOME Shell
  chrome, so the panel and indicator remain readable and clickable.
- **"Show in taskbar" is disabled.** GTK4/Wayland exposes no way to hide or show a
  window in the taskbar independently. The preference is still stored for file
  compatibility, and the row explains the limitation.
- **"Reset window position" resets size and presents the window.** Wayland
  compositors own window placement, so there is no global bottom-right position to
  restore. Header-bar dragging is available when Movable window is on.
- **Monitor values are preserved across hot-plug** rather than deleting the whole
  settings file, unlike the original.
- **Non-dimmable window defaults to off**, matching the original 2.0.1 setting.
  Enable it on the Screens tab if you want the hole around the settings window.

## Troubleshooting

See [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md) for extension-load,
dimming, settings, autostart and packaging problems.

## Project layout

```
dimmer/
├── crates/dimmer-core/       pure model, settings IO, dim maths, autostart
├── crates/dimmer/            GTK4 UI, D-Bus service, lifecycle, CLI
├── gnome-extension/
│   ├── dimmer@dimmer.linux/  Shell extension
│   └── tests/                gjs, headless Shell and two-monitor harnesses
├── gui-tests/                WayDriver GUI suites (standalone workspace)
├── data/                     desktop file, icon, AppStream metainfo
├── docs/                     architecture and troubleshooting
└── scripts/                  install, uninstall, pack
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for design details.

## Credits and license

Dimmer for Linux is an independent reimplementation inspired by **Dimmer 2.0.1**
by Nelson Pires (<https://www.nelsonpires.com/software/dimmer>). The original
application and its assets remain the property of their author. The Linux port
does not include the original application's source or assets.

This port is released under the **MIT License**. See [`LICENSE`](LICENSE).
