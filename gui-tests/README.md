# Dimmer GUI tests

End-to-end tests for the GTK4 application, driven through
[WayDriver](https://crates.io/crates/waydriver): a headless Mutter compositor,
real synthesized input, PipeWire screenshots and AT-SPI accessibility queries.

This is a **standalone Cargo workspace** on purpose. WayDriver needs the
GStreamer development packages, and those must not become part of the main
`cargo test` gate.

## Requirements (Fedora)

```bash
sudo dnf install \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugins-pipewire \
  mutter pipewire wireplumber dbus-daemon at-spi2-core
```

If the development RPMs are only available as extracted files, point
`DIMMER_GST_DEVEL` at the sysroot that contains `usr/lib64/pkgconfig`.

## Run

```bash
cargo build --release
DIMMER_BIN=$PWD/target/release/dimmer ./gui-tests/run.sh
```

Run one suite or a subset:

```bash
DIMMER_BIN=$PWD/target/release/dimmer ./gui-tests/run.sh screens
DIMMER_BIN=$PWD/target/release/dimmer ./gui-tests/run.sh header pages
```

`run.sh` starts each suite in its own private D-Bus session and
`XDG_RUNTIME_DIR`, so the tests never touch the developer's desktop,
`Dimmer.ini` or autostart directory. `donate` is isolated because WayDriver's
mock portal must own `org.freedesktop.portal.Desktop` before Dimmer starts.

`RUST_TEST_NOCAPTURE=1` shows `eprintln!` diagnostics from the tests.

## Suites

| Suite | Covers |
| --- | --- |
| `header` | Every header tab switches with real pointer clicks; Minimise hides the window and `ShowWindow` restores it; Close quits the app. Regression coverage for the drag gesture that used to steal presses from the tabs and end-cap buttons. |
| `pages` | All four tabs expose named, focusable controls; Tab traversal reaches every control and never loses focus; the extension banner's Details dialog opens and closes with Escape. |
| `screens` | The per-screen slider, `+`/`−` step buttons and numeric entry stay in sync; typed values move the slider; `500` clamps to `90`. |
| `autostart` | "Start minimised" and "Auto run" create and remove the XDG autostart entry with `--minimised`. |
| `donate` | The heart button opens the PayPal URL through the portal. |

## Layout

- `tests/common/mod.rs` — session harness: headless Mutter, isolated config,
  D-Bus helpers, pointer click, polling helpers.
- `tests/*.rs` — one suite per binary. WayDriver's `Locator` holds a session
  handle, so tests inside a suite are serialized with a shared mutex.
- `run.sh` — per-suite private bus/runtime wrapper.
- `run-session.sh` — starts the AT-SPI bus launcher/registry when Fedora's
  SELinux policy blocks D-Bus activation inside the nested session, then runs
  one `cargo test --test <suite>`.

## Harness notes

- **AT-SPI depth:** WayDriver caps the accessibility tree at depth 20. Dimmer's
  content sits at depth 20–21 below libadwaita's chrome, so interactive
  controls must stay direct children of the card row. `scale_and_spin` is a
  `GtkGrid` for this reason; wrapping the buttons in another box hides them
  from the harness.
- **Duplicate nodes:** libadwaita exposes each page subtree twice under two
  `TabPanel` parents. A single-target locator rejects the duplicate, so use
  `.first()` or `wait_for_present()` where a control may match twice.
- **GTK FlowBox extents:** GTK reports window-relative extents for widgets
  inside a `GtkFlowBox` about 20 px away from where they are drawn. A pointer
  click at the reported centre misses the 34 px `+`/`−` buttons, so `screens`
  drives them through their AT-SPI `Action` (what a screen reader invokes).
  Pointer routing itself is covered by `header`.
- **GtkEntry focus:** GTK 4.22 does not expose `Component::grab_focus` on
  `GtkEntry`. `screens` tabs to the entry and then uses
  `fill_assume_focused`.
- **Minimise:** headless Mutter ignores `window.minimize()`, so the minimise
  test uses the `ShowInTaskbar=false` hide path instead.
