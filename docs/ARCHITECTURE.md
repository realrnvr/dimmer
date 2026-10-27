# Architecture

## 1. Why a GNOME extension is part of a Rust app

Dimmer for Linux targets GNOME on Wayland, with Fedora Workstation as the
development and reference system. The initial extension metadata supports GNOME
Shell 45–50; other desktop environments and X11 are outside this release's scope.

Wayland deliberately prevents one application from drawing over, positioning itself
over, or observing other applications. GNOME Shell does not implement
`wlr-layer-shell` for third-party clients either. The only supported way to dim
*other* applications on GNOME Wayland is to render a compositor-side overlay from a
GNOME Shell extension.

Therefore the project is split into:

| Component | Language | Responsibility |
|---|---|---|
| `dimmer` | Rust + GTK4/libadwaita | UI, state, validation, persistence, diagnostics, D-Bus service |
| `dimmer@dimmer.linux` | GJS (GNOME Shell) | Draw overlays, IDs, boundaries; reset the app window; top-bar indicator |
| `dimmer-core` | Rust | Pure settings/model/math logic (unit-testable, no GTK) |

The Rust app remains the source of truth for settings and reset requests. The
extension applies GNOME-specific window placement and renders the published
overlay state. This mirrors the proven SpotlightDimmer GNOME adapter design (MIT).

## 2. Process / component diagram

```
                     ┌─────────────────────────────────────────────┐
  user input ───────▶│ dimmer (Rust/GTK4)                          │
                     │                                             │
                     │ AppState ──▶ SettingsStore (Dimmer.ini)     │
                     │    │                                        │
                     │    ├──▶ GTK widgets (screens/options/about) │
                     │    ├──▶ D-Bus snapshot (Arc<RwLock<..>>)    │
                     │    └──▶ StateChanged signal ────────────┐   │
                     └──────────────────────────────┬─────────┼───┘
                                                    │ D-Bus   │
                     ┌──────────────────────────────▼─────────▼───┐
                     │ GNOME Shell extension                      │
                     │                                            │
                     │ DbusClient ──▶ OverlayManager              │
                     │                  ├── St.Widget per monitor │
                     │                  ├── ID labels             │
                     │                  ├── boundary borders      │
                     │                  └── non-dimmable hole     │
                     │                                            │
                     │ Indicator (panel menu) ──▶ D-Bus commands  │
                     └────────────────────────────────────────────┘
```

## 3. D-Bus contract

- **Bus:** session
- **Name:** `io.github.dimmer.Linux`
- **Object path:** `/io/github/dimmer/Linux`
- **Interface:** `io.github.dimmer.Linux`
- **Protocol version:** `1`

### Methods

| Method | Signature | Description |
|---|---|---|
| `Ping` | `() → s` | Returns `"dimmer <version> protocol=1"` |
| `GetState` | `() → s` | Current overlay state as JSON (see below) |
| `RegisterExtension` | `(s extension_version) → s` | Extension announces itself; returns state JSON. Sets `extension_connected=true` |
| `ShowWindow` | `() → ()` | Show + present the settings window |
| `HideWindow` | `() → ()` | Hide the settings window |
| `ResetWindowPosition` | `() → ()` | Restore the default size and center the window on the primary monitor |
| `WindowResetCompleted` | `(s serial, b success) → ()` | GNOME Shell reports the confirmed reset result to the app |
| `SetMaster` | `(b on, d dim) → ()` | Update master quickly (indicator menu) |
| `SetScreen` | `(i id, b on, d dim) → ()` | Update one screen quickly |
| `ExtensionDisconnected` | `() → ()` | Extension disabling itself; clears `extension_connected` without hiding overlays logically |
| `Quit` | `() → ()` | Save and quit the application |

Methods that change settings update the Rust model, persist it, update the UI, and
emit `StateChanged`. Window reset completion is reported separately so the UI can
show its final success or failure toast.

### Signals

| Signal | Signature | Description |
|---|---|---|
| `StateChanged` | `(s state_json)` | Emitted on overlay changes; `window_reset_serial` increments when the extension should reset the app window |
| `VisibilityChanged` | `(b visible)` | Main window shown/hidden; lets the extension update the hole |

### State JSON

```json
{
  "protocol": 1,
  "app_version": "1.0.0",
  "extension_connected": true,
  "window_visible": true,
  "window_app_id": "io.github.dimmer.Linux",
  "window_wm_class": "dimmer",
  "window_reset_serial": 0,
  "show_ids": false,
  "show_boundaries": false,
  "non_dimmable_window": true,
  "master": { "on": false, "dim": 30.0 },
  "screens": [
    {
      "id": 1,
      "on": true,
      "dim": 30.0,
      "tint": "000000",
      "connector": "eDP-1",
      "geometry": { "x": 0, "y": 0, "width": 1920, "height": 1080 },
      "scale": 1.0,
      "primary": true
    }
  ]
}
```

The extension matches screens to Mutter monitors by `connector` first, then by
geometry, then by sorted index. `dim` is the **effective** dim percentage after
master override. Tint is applied as the overlay RGB; alpha is `dim / 100`.

## 4. Settings file

Path resolution order:

1. `--config <path>` (CLI)
2. `Dimmer.ini` next to the executable, if it exists and is writable (portable)
3. `$XDG_CONFIG_HOME/dimmer/Dimmer.ini`
4. `~/.config/dimmer/Dimmer.ini`

Format is the original JSON shape (compatible field names plus optional Linux
fields ignored by the original):

```json
{
  "Position": { "Left": null, "Top": null },
  "ShowIds": false,
  "ShowBoundaries": false,
  "AutoRun": { "On": false, "AllUsers": false, "Mode": "StartupFolder" },
  "MovableWindow": true,
  "StartMinimised": false,
  "NonDimmableWindow": true,
  "CloseHidesToTray": false,
  "ShowInTaskbar": true,
  "Master": { "On": false, "Dim": 30.0 },
  "Screens": [
    { "Id": 1, "On": true, "Dim": 30.0, "Tint": "000000" }
  ],
  "LastRun": "2026-10-25 23:00:00"
}
```

Writes are atomic (temp file + rename). Malformed files are backed up to
`Dimmer.ini.bak-<timestamp>` and regenerated, with the error surfaced in Debug.

## 5. Overlay semantics

- One overlay root actor lives in `global.window_group`, i.e. above normal windows
  but below Shell chrome (`uiGroup` children such as the panel, overview, lock
  screen, menus and modal-dialog lightboxes). `set_child_above_sibling(root, null)`
  re-raises it after Mutter restacks window actors.
- The root contains one group per monitor. Each group has a rectangle container, a
  boundary actor and an ID label. All actors are `reactive = false`,
  `can_focus = false`, `track_hover = false` → click-through.
- Screen IDs: 128 px bold white labels at the top-left of each monitor.
- Boundaries: 5 px pure-yellow border on each monitor.
- Tint: RGB from the screen setting; alpha = effective dim / 100.
- Non-dimmable window: when enabled and the settings window is visible, every
  monitor that overlaps the window frame is split into up to four rectangles around
  it (found by app id/WM class in `global.get_window_actors()` and refreshed every
  150 ms while visible). The window itself is therefore shown at full brightness.
- Overlays are hidden while the overview is open and restored when it closes.
- `trackFullscreen: false` so fullscreen apps on one monitor do not hide overlays
  on other monitors.
- Compositor unredirect is disabled while the extension is active (GNOME 47+:
  `global.compositor.disable_unredirect()`; older: `Meta.disable_unredirect_for_display`).
- Because the overlay is below Shell chrome, Shell modal dialogs (for example the
  first-login `WelcomeDialog`) dim it with their lightbox. This is intentional:
  the dialog must stay readable and on top.

## 6. Lifecycle

- The app owns the D-Bus name. A second launch sees the name taken, calls
  `ShowWindow` on the existing instance, and exits.
- Dimming remains active while the window is hidden/minimised.
- State changes from D-Bus and the extension are delivered to the GTK thread as
  `UiCommand` values over an unbounded Tokio channel and dispatched with
  `glib::MainContext::default().spawn_local`. The shared snapshot is updated under
  its lock first; there is no polling timer, so the app is fully idle when nothing
  changes.
- The extension watches the bus name; if the app dies, all overlays are hidden.
- When the extension is disabled it calls `ExtensionDisconnected`, removes every
  actor, signal subscription and timeout, and clears `extension_connected` in the app.
- The top-bar indicator exists even when the app is not running and offers
  "Start Dimmer" (launches the desktop file).
- `CloseHidesToTray`: close button hides instead of quitting.
- "Quit" in the indicator / app menu always quits.
- Autostart is an XDG `.desktop` file in `~/.config/autostart`, honoring
  `StartMinimised` via `--minimised`.

## 7. Files installed

`scripts/install.sh` builds and installs a native per-user copy under `$PREFIX`
(default `~/.local`). `scripts/install-flatpak.sh` installs the Flathub app and
the host extension for the current user; a Flatpak app cannot write the extension
into the host GNOME Shell directory.
The GNOME Shell extension is embedded in the binary; `dimmer --install-extension`
writes it into the XDG data directory and is also called by the installer.

| Path | Purpose |
|---|---|
| `~/.local/bin/dimmer` | release binary |
| `~/.local/share/applications/io.github.dimmer.Linux.desktop` | launcher |
| `~/.local/share/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg` | icon |
| `~/.local/share/metainfo/io.github.dimmer.Linux.metainfo.xml` | AppStream data |
| `~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux/` | shell extension |
| `~/.config/dimmer/Dimmer.ini` | settings (created on first run) |
| `~/.config/autostart/io.github.dimmer.Linux.desktop` | optional autostart |

`scripts/uninstall.sh` removes all of the above except `Dimmer.ini`; `--purge`
removes the settings directory too. Neither script needs root or modifies system
directories.

## 8. References

- Original behaviour reference: Dimmer 2.0.1 by Nelson Pires, documented at
  https://www.nelsonpires.com/software/dimmer. This repository does not contain
  the original application's source or assets.
- Soft Brightness GNOME extension (overlay via `St.Widget`, opacity):
  https://github.com/F-i-f/soft-brightness
- OLED Dimmer (GNOME overlay effect):
  https://github.com/fiji-flo/oled_dimmer
- SpotlightDimmer (Rust daemon + GNOME adapter + D-Bus):
  https://github.com/thomazmoura/spotlight-dimmer
- gtk4-rs: https://github.com/gtk-rs/gtk4-rs
- libadwaita-rs: https://gitlab.gnome.org/World/Rust/libadwaita-rs
- zbus: https://github.com/z-galaxy/zbus
- GJS extension guide: https://gjs.guide/extensions/
