#!/usr/bin/env bash
# Two-monitor check in a private headless GNOME Shell.  Verifies that the app
# reconciles two outputs, the extension builds two overlay groups, and screen /
# master changes reach the right monitor only.
#
# Run as: dbus-run-session -- bash gnome-extension/tests/multimon-shell.sh
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIMMER_BIN=${DIMMER_BIN:-$ROOT/target/debug/dimmer}
BASE=/tmp/opencode/multimon-shell
LOG=$BASE/log
rm -rf "$BASE"
mkdir -p "$BASE"/{data,config,cache,runtime,dimmer-config,log}
chmod 700 "$BASE/runtime"
mkdir -p "$BASE/data/gnome-shell/extensions"
cp -r "$ROOT/gnome-extension/dimmer@dimmer.linux" \
    "$BASE/data/gnome-shell/extensions/dimmer@dimmer.linux"

mkdir -p "$BASE/dimmer-config/dimmer"
cat >"$BASE/dimmer-config/dimmer/Dimmer.ini" <<'JSON'
{
  "ShowIds": true,
  "ShowBoundaries": true,
  "NonDimmableWindow": false,
  "Master": { "On": false, "Dim": 30.0 },
  "Screens": [
    { "Id": 1, "On": true, "Dim": 40.0, "Tint": "000000" },
    { "Id": 2, "On": true, "Dim": 60.0, "Tint": "000000" }
  ]
}
JSON

export XDG_DATA_HOME=$BASE/data
export XDG_CONFIG_HOME=$BASE/config
export XDG_CACHE_HOME=$BASE/cache
export XDG_RUNTIME_DIR=$BASE/runtime
export WAYLAND_DISPLAY=wayland-dimmer
export LIBGL_ALWAYS_SOFTWARE=1
export MUTTER_DEBUG_DUMMY_MODE_SPECS=1920x1080

SHELL_PID=
DIMMER_PID=

cleanup() {
    [ -n "${DIMMER_PID:-}" ] && kill "$DIMMER_PID" 2>/dev/null
    [ -n "${SHELL_PID:-}" ] && kill "$SHELL_PID" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

gnome-shell --headless --unsafe-mode \
    --virtual-monitor 1920x1080 --virtual-monitor 1280x720 \
    --wayland-display=wayland-dimmer >"$LOG/shell.log" 2>&1 &
SHELL_PID=$!
for _ in $(seq 1 100); do
    if gdbus introspect --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
echo "shell started (pid $SHELL_PID)"

for _ in $(seq 1 40); do
    gnome-extensions enable dimmer@dimmer.linux >/dev/null 2>&1 || true
    if gnome-extensions info dimmer@dimmer.linux 2>/dev/null |
            grep -q 'State: ACTIVE'; then
        break
    fi
    sleep 0.25
done
gnome-extensions info dimmer@dimmer.linux 2>&1 | grep -E 'State|Version' || true

XDG_CONFIG_HOME=$BASE/dimmer-config \
    WAYLAND_DISPLAY=wayland-dimmer \
    "$DIMMER_BIN" --show >"$LOG/dimmer.log" 2>&1 &
DIMMER_PID=$!

# Wait for the app to reconcile both Mutter outputs and the extension to connect.
for _ in $(seq 1 60); do
    if gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>/dev/null |
            grep -q '"extension_connected":true'; then
        if gdbus call --session --dest io.github.dimmer.Linux \
            --object-path /io/github/dimmer/Linux \
            --method io.github.dimmer.Linux.GetState 2>/dev/null |
                grep -q '"id":2'; then
            break
        fi
    fi
    sleep 0.25
done
sleep 1.5

# Leave the Fedora first-login modal/overview out of the way.
gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'Main.overview.hide(); Main.layoutManager.modalDialogGroup.get_children().forEach(c => c.hide()); "ok"' \
    >/dev/null 2>&1 || true
sleep 0.5

gjs -m "$ROOT/gnome-extension/tests/multimon-check.js"
status=$?

echo "--- shell log (extension lines) ---"
grep -iE 'dimmer|JS ERROR|CRITICAL' "$LOG/shell.log" | tail -20 || true
exit $status
