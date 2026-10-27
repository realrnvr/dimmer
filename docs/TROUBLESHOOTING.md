# Troubleshooting

Start here:

```bash
dimmer --version
dimmer --debug          # verbose log on stderr
gnome-extensions info dimmer@dimmer.linux
```

The Debug tab also shows whether the extension is connected, the monitor list, the
settings path and the current settings JSON.

Flatpak users can install or update the app and host extension from a source
checkout with `scripts/install-flatpak.sh`. The app itself cannot write its
extension into the host GNOME Shell directory.

## The extension is not loaded or not active

`gnome-extensions info dimmer@dimmer.linux` should report `State: ACTIVE`.

GNOME Shell scans an extension directory only at login. After the first install:

```bash
# log out and back in, then
gnome-extensions enable dimmer@dimmer.linux
```

### Updating an extension that is already loaded

GNOME Shell imports an extension's ES modules once per session. Disabling and
enabling it reuses the cached modules, so changed JS files are not picked up even
though the extension still reports `ACTIVE`. The Shell only notices a different
`metadata.json` version and then asks for a logout. After updating an
already-loaded extension, log out and back in (or reboot), then check:

```bash
gnome-extensions info dimmer@dimmer.linux   # Version: 3
```

The app's Debug tab also shows the connected extension version. If it still
shows the old number, the old module is still cached in the running Shell.

If the state is `ERROR`, read the extension log:

```bash
journalctl --user -b -o cat /usr/bin/gnome-shell | grep -iE 'dimmer|JS ERROR'
```

Common causes:

- The extension directory is missing files. For a native install, reinstall with
  `dimmer --install-extension`; for Flatpak, use `scripts/install-flatpak.sh`.
  Check that all seven files are present:
  `metadata.json`, `extension.js`, `dbusClient.js`, `indicator.js`,
  `overlayManager.js`, `overlayMath.js`, `stylesheet.css`.
- A stale copy exists in another XDG data directory. Check
  `$XDG_DATA_HOME/gnome-shell/extensions/dimmer@dimmer.linux` and
  `~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux`.
- The Shell version is outside 45–50. `metadata.json` declares the supported
  range; a future Shell may need a compatibility update.
- The session is X11 or another compositor. This port targets GNOME on Wayland.

`gnome-extensions install` sometimes fails with
`Can’t recursively copy directory` on Fedora even for a valid zip. Prefer
`dimmer --install-extension`, or unpack the zip manually into
`$XDG_DATA_HOME/gnome-shell/extensions/dimmer@dimmer.linux/`.

## The extension is active but nothing is dimmed

1. Check that the app is running:

   ```bash
   gdbus call --session --dest io.github.dimmer.Linux \
     --object-path /io/github/dimmer/Linux \
     --method io.github.dimmer.Linux.GetState
   ```

   The JSON should contain `"extension_connected":true`.

2. Check that at least one screen has `"on": true` and a dim above 0. The master
   control only applies when it is on; with master off each screen uses its own
   value.

3. Turn a slider to 50 and look at the debug log for `StateChanged`. If the state
   changes but the overlay does not, the extension is not receiving signals.
   Disable and re-enable it; the client re-registers on enable.

4. If `extension_connected` is false, the extension never reached the service.
   Restart both sides: `dimmer --quit && dimmer`.

## Only one monitor is available

The app reads monitors from GDK/Mutter. On a headless or remote session there may
be only one. Use the two-monitor harness to validate the multi-monitor code path
without extra hardware:

```bash
DIMMER_BIN=$PWD/target/release/dimmer timeout 180 \
  dbus-run-session -- bash gnome-extension/tests/multimon-shell.sh
```

It starts its own headless Shell with two virtual monitors and does not affect the
real desktop.

## The settings window is dimmed, or the hole is in the wrong place

- Enable **Non-dimmable window** on the Screens tab. It is off by default, matching
  the original 2.0.1 setting.
- The hole is refreshed while the window is visible. Moving or resizing the window
  should update it within about 150 ms.
- If the window is maximised or spans monitors, every overlapping monitor is split
  around the frame, so the hole can cover a large area. That is expected.
- The overlay is below GNOME Shell modals. A Shell dialog (for example the Fedora
  Welcome tour) draws a lightbox over everything, including the dim overlay. Close
  the dialog.

## The top bar/panel is not dimmed

Intentional. Overlays live above normal windows but below GNOME Shell chrome, so
the panel, menus, overview and lock screen stay readable and usable. Dimming the
panel would also dim the emergency/lock UI.

## Window placement or taskbar behaviour

Wayland applications cannot position their own windows. **Reset window position**
asks the GNOME Shell extension to restore the default size and center the window
in the primary monitor's work area. Header-bar dragging works when **Movable
window** is on.

**Show in taskbar** is stored for settings compatibility but disabled in the UI:
GTK4/Wayland has no API to independently show or hide a window in the taskbar.
Minimise behaves according to the same preference row and its explanation.

## Close does not quit

If **Close hides to tray** is on, the close button hides the window and dimming
continues. Reopen from the top-bar indicator or run `dimmer --show`. To really
quit, use the indicator's Quit item or `dimmer --quit`. Turning the option off
restores close-to-quit.

## A second launch does nothing

That is the single-instance design: the second process asks the running instance
to show its window and exits. If the running instance is hung, quit it explicitly:

```bash
dimmer --quit
```

`--quit` with no running instance exits after a short D-Bus timeout (about two
seconds) with a non-zero status; that is not a crash.

## Settings problems

Locate the file in the Debug tab. Resolution order is `--config`, a portable
`Dimmer.ini` next to the binary, `$XDG_CONFIG_HOME/dimmer/Dimmer.ini`, then
`~/.config/dimmer/Dimmer.ini`.

- **Corrupt JSON:** the app backs the file up as
  `Dimmer.ini.bak-<timestamp>`, writes defaults, and reports the error in Debug.
- **Unwritable path:** start with a writable file, for example
  `dimmer --config /tmp/Dimmer.ini`. The app reports the error instead of
  crashing.
- **Values out of range:** dims are clamped to 0–90; invalid tints fall back to
  `000000`; NaN/infinity is rejected.
- **Copying from Windows:** field names match the original. Unknown fields are
  ignored.
- **Reset everything:** quit Dimmer, move `Dimmer.ini` aside, start again.

## Autostart

Auto run creates `$XDG_CONFIG_HOME/autostart/io.github.dimmer.Linux.desktop`
(usually `~/.config/autostart/...`). If it does not start at login:

```bash
cat ~/.config/autostart/io.github.dimmer.Linux.desktop
```

- The entry includes the absolute binary path. Reinstall or toggle Auto run after
  moving the binary.
- With **Start minimised** on, the entry passes `--minimised`; without it, the
  window opens at login.
- The desktop must be a GNOME/Wayland session and the binary must still exist.
- Toggling the option off removes only Dimmer's own entry.

The **Auto run** switch is unavailable in the Flatpak build because it cannot
write to the host autostart directory. Use your desktop's startup application
settings to add `flatpak run io.github.dimmer.Linux` instead.

## High CPU or memory

The app is event-driven and should be at 0.000 % CPU when idle. Memory is dominated
by GTK/GSK and the graphics driver; about 50 MB RSS is expected when started with
`--minimised`, and a visible window can use more with the Nvidia/Vulkan stack.

The extension polls only the settings-window frame (every 150 ms) while
**Non-dimmable window** is enabled and the window is visible. Turn that option off
to remove the timer.

## Building fails

- `pkg-config` cannot find `gtk4` or `libadwaita-1`: install the development
  packages, or point `PKG_CONFIG_PATH` at the prefix that contains them:

  ```bash
  PKG_CONFIG_PATH=/path/to/lib/pkgconfig cargo build --release
  ```

- Rust is older than 1.85: update the toolchain.
- The linker cannot find GTK: install `gcc`, `pkg-config` and the `-devel`
  packages listed in the README.
- `cargo clippy --all-targets -- -D warnings` must be clean; the workspace denies
  `unwrap`, `expect`, `panic`, `todo` and `dbg!` in production code.

## Testing the extension safely

The harnesses use `dbus-run-session` and private `XDG_*` trees. They do not touch
the real app, settings or Shell:

```bash
gjs -m gnome-extension/tests/overlay-math.test.js

DIMMER_BIN=$PWD/target/release/dimmer timeout 240 \
  dbus-run-session -- bash gnome-extension/tests/headless-shell.sh
```

If the real session bus is used accidentally, stop any test instance first with
`dimmer --quit`.

## Uninstall did not remove everything

```bash
./scripts/uninstall.sh --purge
```

This removes the binary, launcher, icon, metainfo, extension, autostart entry and
`~/.config/dimmer`. If you installed with a custom prefix, pass the same
`--prefix`. Extensions in a non-default `XDG_DATA_HOME` need that directory
removed manually.
