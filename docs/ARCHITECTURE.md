# Architecture

## 1. Why a GNOME extension is part of a Rust app

Dimmer for Linux targets GNOME on Wayland, with Fedora Workstation as the
development and reference system. The extension metadata supports GNOME Shell
45–51 — every release of the ESModule extension API through the current stable
shell — while other desktop environments and X11 remain outside this release's
scope.

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
| `SetAdaptive` | `(b on, d strength) → ()` | Turn adaptive dimming on/off and set its strength |
| `ReportAdaptiveMetrics` | `(s json) → ()` | Extension reports live adaptive measurements at 2 Hz; malformed payloads are ignored |
| `GetAdaptiveMetrics` | `() → s` | Last adaptive payload as JSON, or `null` before the first report (diagnostics) |
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
  "adaptive": { "on": false, "strength": 50.0 },
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
  "Adaptive": { "On": false, "Strength": 50.0 },
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
- Compositor unredirect is disabled while the extension is active. The shell
  API is feature-detected: `global.compositor.disable_unredirect()` is used where
  available, with the legacy `Meta.disable_unredirect_for_display()` path
  retained as a fallback for older releases.
- The extension targets the stable ESM API across GNOME 45–51. GNOME 51 removed
  the `vertical` St property and rejects an async `disable()`; neither is used
  here. The legacy `Meta.disable_unredirect_for_display()` helpers are absent
  from current releases, so the compositor API is preferred and the old call is
  only a feature-detected fallback. Popup-menu calls use the no-argument form,
  valid both before and after GNOME 51's parameter-object change.
  `metadata.json` declares only released shell versions, so the next development
  cycle is added after its API freezes.
- Because the overlay is below Shell chrome, Shell modal dialogs (for example the
  first-login `WelcomeDialog`) dim it with their lightbox. This is intentional:
  the dialog must stay readable and on top.

### Adaptive dimming

Optional, off by default, and global to every screen. It only ever adds darkness
on top of the user's chosen dim; it cannot brighten the screen or change the tint.

- **Layer.** Each screen gets a second rectangle group of pure black, above the
  tint rectangles and below the boundary and ID label. The user's tint
  compositing therefore stays bit-identical, and the extra darkness is
  independent of the tint colour.
- **Sampling.** Four times a second (250 ms × `ceil(screens/2)`) the extension
  calls `Clutter.Stage.paint_to_content()` synchronously for one monitor at a
  fixed 32 400-pixel budget (240×135 at 16:9), then decodes the result through
  `Shell.Screenshot.composite_to_stream()` with GdkPixbuf. Sampling covers the
  non-dimmable hole and the panel; those regions are excluded by a mask because
  neither carries the adaptive layer. Ten samples a second drive the actor; the
  cost measured on GNOME 50.6 is 2–10 ms per full sample.
- **Statistics.** A 256-bin luma histogram over the masked pixels yields the
  arithmetic mean `M` and the mean of the brightest 20 % `H`. Both are encoded
  sRGB fractions in `[0, 1]`, the space Mutter actually blends in, so the model
  below is exact rather than perceptual guesswork.
- **Feedback.** The capture includes the adaptive layer already applied, so the
  controller divides it back out: `x = y / (1 − a_q)`, where `a_q` is the alpha
  quantised to 8 bits, the precision St renders. Scaling preserves ordering, so
  this recovers `H` and `M` exactly.
- **Control law.** Strength `s ∈ [0, 1]` sets two ceilings, `H_target = 0.72 −
  0.20 s` and `M_target = 0.42 − 0.14 s`. The smallest alpha satisfying both is
  `a = min(max(1 − H_target/H, 1 − M_target/M), headroom)`, with `headroom =
  max(0, min(0.50, 0.90 − user_dim))`. Adaptive darkness is capped at 0.50 and
  the total at 0.90; at 90 % user dim there is no headroom and the layer stays
  off. Because one scalar multiply scales both statistics, taking the maximum of
  the two requirements is exact, not a tuning choice.
- **Dynamics.** Attack 0.80/s, release 0.10/s, hysteresis 0.02, settle 0.005.
  Darkening is visible immediately; lightening is deliberately slow so a passing
  bright window does not flash.
- **Pausing.** Overview, lock screen, modal dialogs and open panel menus freeze
  the layer instead of releasing it, so nothing jumps behind Shell chrome.
- **Failure handling.** Three consecutive capture failures disable the feature
  for the session, clear the layer, and report `available: false`. No pixels are
  written to disk, kept between samples, or sent anywhere; only the two
  statistics and the applied alpha travel over D-Bus.

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

`scripts/install.sh` builds and installs a per-user copy under `$PREFIX`
(default `~/.local`). GitHub releases provide the same layout through a prebuilt
binary and `scripts/install-release.sh`.
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
