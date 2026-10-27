#!/usr/bin/env bash
# Run a private headless GNOME Shell, install the extension into a temporary
# data dir, and check that it loads and talks to the Dimmer app.  Intended to
# be run as: dbus-run-session -- bash gnome-extension/tests/headless-shell.sh
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIMMER_BIN=${DIMMER_BIN:-$ROOT/target/debug/dimmer}
EXTENSION_BUNDLE=${EXTENSION_BUNDLE:-}
BASE=/tmp/opencode/nested-shell
LOG=$BASE/log
rm -rf "$BASE"
mkdir -p "$BASE"/{data,config,cache,runtime,dimmer-config,log}
chmod 700 "$BASE/runtime"
mkdir -p "$BASE/data/gnome-shell/extensions"
if [ -n "$EXTENSION_BUNDLE" ]; then
    # Test the actual distributable, not just the source tree.  We extract it
    # ourselves because gnome-extensions install is broken on some hosts
    # (autoar reports "Can't recursively copy directory").
    mkdir -p "$BASE/data/gnome-shell/extensions/dimmer@dimmer.linux"
    unzip -q "$EXTENSION_BUNDLE" \
        -d "$BASE/data/gnome-shell/extensions/dimmer@dimmer.linux"
else
    cp -r "$ROOT/gnome-extension/dimmer@dimmer.linux" \
        "$BASE/data/gnome-shell/extensions/dimmer@dimmer.linux"
fi

mkdir -p "$BASE/dimmer-config/dimmer"
cat >"$BASE/dimmer-config/dimmer/Dimmer.ini" <<'JSON'
{
  "ShowIds": true,
  "ShowBoundaries": true,
  "NonDimmableWindow": true,
  "StartMinimised": false,
  "Master": { "On": false, "Dim": 30.0 },
  "Screens": [ { "Id": 1, "On": true, "Dim": 40.0, "Tint": "000000" } ]
}
JSON
cat "$BASE/dimmer-config/dimmer/Dimmer.ini"

export XDG_DATA_HOME=$BASE/data
export XDG_CONFIG_HOME=$BASE/config
export XDG_CACHE_HOME=$BASE/cache
export XDG_RUNTIME_DIR=$BASE/runtime
export WAYLAND_DISPLAY=wayland-dimmer
export LIBGL_ALWAYS_SOFTWARE=1
export MUTTER_DEBUG_DUMMY_MODE_SPECS=1920x1080

# Give the private session its own HOME so the indicator cannot fall back to a
# real ~/.local/bin/dimmer and accidentally pass the Start Dimmer check.
export HOME=$BASE/home
mkdir -p "$HOME"

# Install a per-user desktop entry that points at the test binary.  This is the
# same absolute Exec/TryExec form scripts/install.sh writes, and it is what the
# indicator's Start Dimmer path must resolve.
mkdir -p "$XDG_DATA_HOME/applications"
cat >"$XDG_DATA_HOME/applications/io.github.dimmer.Linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Dimmer
Exec=$DIMMER_BIN
TryExec=$DIMMER_BIN
Icon=io.github.dimmer.Linux
Terminal=false
StartupWMClass=io.github.dimmer.Linux
EOF

SHELL_PID=
DIMMER_PID=

cleanup() {
    [ -n "${DIMMER_PID:-}" ] && kill "$DIMMER_PID" 2>/dev/null
    [ -n "${SHELL_PID:-}" ] && kill "$SHELL_PID" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

start_shell() {
    rm -f "$LOG/shell.log"
    gnome-shell --headless --unsafe-mode --virtual-monitor 1920x1080 \
        --wayland-display=wayland-dimmer >"$LOG/shell.log" 2>&1 &
    SHELL_PID=$!
    for _ in $(seq 1 100); do
        if gdbus introspect --session --dest org.gnome.Shell \
            --object-path /org/gnome/Shell >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
    done
    echo "shell did not start" >&2
    tail -40 "$LOG/shell.log" >&2
    return 1
}

start_shell || exit 1
echo "shell started (pid $SHELL_PID)"

enable_extension() {
    for _ in $(seq 1 40); do
        gnome-extensions enable dimmer@dimmer.linux >/dev/null 2>&1 || true
        if gnome-extensions info dimmer@dimmer.linux 2>/dev/null |
                grep -q 'State: ACTIVE'; then
            return 0
        fi
        sleep 0.25
    done
    return 1
}

enable_extension || {
    echo "extension did not become active" >&2
    gnome-extensions info dimmer@dimmer.linux 2>&1 || true
    exit 1
}
echo "--- extension info ---"
gnome-extensions info dimmer@dimmer.linux 2>&1 || true

# Start the Dimmer app on this private bus/display.
XDG_CONFIG_HOME=$BASE/dimmer-config \
    WAYLAND_DISPLAY=wayland-dimmer \
    "$DIMMER_BIN" --show >"$LOG/dimmer.log" 2>&1 &
DIMMER_PID=$!

for _ in $(seq 1 50); do
    if gdbus introspect --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux >/dev/null 2>&1; then
        break
    fi
    sleep 0.2
done
for _ in $(seq 1 50); do
    if gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>/dev/null |
            grep -q '"extension_connected":true'; then
        break
    fi
    sleep 0.2
done

sleep 2
echo "--- dimmer state ---"
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.GetState 2>&1 | head -c 1200
echo

echo "--- windows ---"
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'JSON.stringify(global.get_window_actors().map(a => { const w = a.meta_window; return {app: w.get_gtk_application_id(), cls: w.get_wm_class(), title: w.get_title(), rect: w.get_frame_rect()}; }))' \
    2>&1 || true

echo "--- reset window position and size ---"
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'global.get_window_actors().map(a => a.meta_window).find(w => w.get_gtk_application_id() === "io.github.dimmer.Linux")?.maximize(); "maximized"' \
    2>&1 || true
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.ResetWindowPosition 2>&1 || true
sleep 0.5
reset_geometry=$(gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell --method org.gnome.Shell.Eval \
    '(() => { const w = global.get_window_actors().map(a => a.meta_window).find(candidate => candidate.get_gtk_application_id() === "io.github.dimmer.Linux"); const i = Main.layoutManager.primaryIndex; const a = Main.layoutManager.getWorkAreaForMonitor(i); const r = w?.get_frame_rect(); if (!r || !a) return false; const width = Math.min(1040, a.width); const height = Math.min(700, a.height); return !w.maximized_horizontally && !w.maximized_vertically && r.width === width && r.height === height && Math.abs(r.x - (a.x + (a.width - width) / 2)) <= 1 && Math.abs(r.y - (a.y + (a.height - height) / 2)) <= 1; })()' \
    2>&1)
echo "reset geometry: $reset_geometry"
if ! echo "$reset_geometry" | grep -q 'true'; then
    echo "FAIL: reset should center and resize the window on the primary work area" >&2
    exit 1
fi
echo "PASS: reset restored the default size and primary-monitor position"

echo "--- leave overview / close welcome tour ---"
# The Fedora "Welcome" tour is a Shell modal dialog (WelcomeDialog).  Its
# lightbox dims everything below it, including our overlay, so close it before
# taking verification screenshots.
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.overview.hide(); Main.layoutManager.modalDialogGroup.get_children().forEach(c => c.hide()); global.get_window_actors().map(a => a.meta_window).forEach(w => { const text = `${w.get_wm_class() ?? ""} ${w.get_title() ?? ""} ${w.get_gtk_application_id() ?? ""}`; if (/gnome-tour|Welcome/i.test(text)) w.delete(global.get_current_time()); }); "ok"' \
    2>&1 || true
sleep 1.5

echo "--- verify label/boundary are not dimmed ---"
gdbus call --session --dest org.gnome.Shell.Screenshot \
    --object-path /org/gnome/Shell/Screenshot \
    --method org.gnome.Shell.Screenshot.Screenshot false false "$BASE/shot-unobstructed.png" \
    2>&1 || true
gjs -m "$ROOT/gnome-extension/tests/screenshot-check.js" \
    "$BASE/shot-unobstructed.png" || true

echo "--- overlay actors ---"
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify(root ? {visible: root.visible, screens: root.get_children().map(s => ({name: s.name, visible: s.visible, layers: s.get_children().map(layer => ({name: layer.name, visible: layer.visible, style: layer.style, rects: layer.get_children().map(r => ({name: r.name, x: r.x, y: r.y, w: r.width, h: r.height, visible: r.visible, style: r.style}))}))}))} : "no root")' \
    2>&1 || true

echo "--- normal screenshot ---"
gdbus call --session --dest org.gnome.Shell.Screenshot \
    --object-path /org/gnome/Shell/Screenshot \
    --method org.gnome.Shell.Screenshot.Screenshot false false "$BASE/shot-normal.png" \
    2>&1 || true

echo "--- master + screen updates ---"
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.SetMaster true 55 2>&1 || true
sleep 0.5
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify(root.get_children()[0].get_children()[0].get_children().map(r => ({visible: r.visible, style: r.style})))' \
    2>&1 || true

gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.SetMaster false 30 2>&1 || true
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.SetScreen 1 false 0 2>&1 || true
sleep 0.5
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify(root.get_children()[0].get_children()[0].get_children().map(r => ({visible: r.visible, style: r.style})))' \
    2>&1 || true

gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.SetScreen 1 true 40 2>&1 || true
sleep 0.5

echo "--- hide window (hole removed) ---"
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.HideWindow 2>&1 || true
sleep 0.7
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.GetState 2>&1 | grep -o '"window_visible":[a-z]*'
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify(root.get_children()[0].get_children()[0].get_children().map(r => ({x: r.x, y: r.y, w: r.width, h: r.height, visible: r.visible, style: r.style})))' \
    2>&1 || true

echo "--- indicator Open shows window again ---"
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.panel.statusArea["dimmer@dimmer.linux"]?._open(); "opened"' 2>&1 || true
sleep 1
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.GetState 2>&1 | grep -o '"window_visible":[a-z]*'
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify(root.get_children()[0].get_children()[0].get_children().map(r => ({x: r.x, y: r.y, w: r.width, h: r.height, visible: r.visible})))' \
    2>&1 || true

echo "--- indicator menu ---"
indicator_menu=$(gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let ind = Main.panel.statusArea["dimmer@dimmer.linux"]; `${Boolean(ind?._masterSwitch)} ${Boolean(ind?._levelMenu)} ${ind?._statusItem?.visible} ${ind?.menu?._getMenuItems?.().map(i => i.label?.text ?? "").join("|")}`' \
    2>&1)
echo "indicator menu: $indicator_menu"
if echo "$indicator_menu" | grep -q '^.*false false false ' &&
        echo "$indicator_menu" | grep -q 'Dimmer is not running' &&
        echo "$indicator_menu" | grep -q 'Open Dimmer' &&
        echo "$indicator_menu" | grep -q 'Reset window position' &&
        echo "$indicator_menu" | grep -q 'Quit Dimmer' &&
        ! echo "$indicator_menu" | grep -q 'Master'; then
    echo "PASS: panel is launcher/status only"
else
    echo "FAIL: panel menu should be status/Open/Reset/Quit only" >&2
    exit 1
fi

gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.panel.statusArea["dimmer@dimmer.linux"]?.menu.open(); "menu"' \
    2>&1 || true
sleep 0.7
gdbus call --session --dest org.gnome.Shell.Screenshot \
    --object-path /org/gnome/Shell/Screenshot \
    --method org.gnome.Shell.Screenshot.Screenshot false false "$BASE/shot-menu.png" \
    2>&1 || true
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.panel.statusArea["dimmer@dimmer.linux"]?.menu.close(); "closed"' \
    2>&1 || true

echo "--- overview hides overlay ---"
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.overview.show(); "shown"' 2>&1 || true
sleep 1
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let root = global.window_group.get_children().find(a => a.name === "dimmer-overlay-root"); JSON.stringify({overview: Main.overview.visible, rootVisible: root?.visible})' \
    2>&1 || true
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.overview.hide(); "hidden"' 2>&1 || true

echo "--- disable extension ---"
gnome-extensions disable dimmer@dimmer.linux 2>&1 || true
sleep 1
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'JSON.stringify({root: Boolean(global.window_group.get_children().find(a => a.name === "dimmer-overlay-root")), indicator: Boolean(Main.panel.statusArea["dimmer@dimmer.linux"])})' \
    2>&1 || true
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.GetState 2>&1 | grep -o '"extension_connected":[a-z]*'

echo "--- re-enable extension ---"
enable_extension || true
sleep 0.5
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'JSON.stringify({root: Boolean(global.window_group.get_children().find(a => a.name === "dimmer-overlay-root")), indicator: Boolean(Main.panel.statusArea["dimmer@dimmer.linux"])})' \
    2>&1 || true
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.GetState 2>&1 | grep -o '"extension_connected":[a-z]*'

echo "--- indicator Start Dimmer after quit ---"
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.Quit 2>&1 || true
for _ in $(seq 1 60); do
    if ! gdbus introspect --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
sleep 0.5
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'let ind = Main.panel.statusArea["dimmer@dimmer.linux"]; `${ind?._connected} ${ind?._openItem?.label?.text}`' \
    2>&1 || true
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.panel.statusArea["dimmer@dimmer.linux"]?._open(); "start"' 2>&1 || true
started=0
for _ in $(seq 1 100); do
    if gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>/dev/null |
            grep -q '"window_visible":true'; then
        started=1
        break
    fi
    sleep 0.2
done
if [ "$started" -eq 1 ]; then
    echo "PASS: indicator Start Dimmer launched the app"
else
    echo "FAIL: indicator Start Dimmer did not launch the app" >&2
    grep -iE 'dimmer|error|JS' "$LOG/shell.log" | tail -20 >&2 || true
    exit 1
fi
reconnected=0
for _ in $(seq 1 50); do
    panel_state=$(gdbus call --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Eval \
        'let ind = Main.panel.statusArea["dimmer@dimmer.linux"]; `${ind?._connected} ${ind?._openItem?.label?.text}`' \
        2>/dev/null)
    if echo "$panel_state" | grep -q 'true Open Dimmer'; then
        reconnected=1
        break
    fi
    sleep 0.2
done
if [ "$reconnected" -eq 1 ]; then
    echo "PASS: panel reconnected after Start Dimmer"
else
    echo "FAIL: panel did not reconnect after Start Dimmer" >&2
    exit 1
fi
gdbus call --session --dest io.github.dimmer.Linux \
    --object-path /io/github/dimmer/Linux \
    --method io.github.dimmer.Linux.Quit 2>&1 || true

echo "--- shell log (extension lines) ---"
grep -iE 'dimmer|extension|error|JS' "$LOG/shell.log" | tail -40 || true

echo "--- dimmer log ---"
tail -20 "$LOG/dimmer.log" || true
