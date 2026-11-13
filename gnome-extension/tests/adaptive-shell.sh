#!/usr/bin/env bash
# Adaptive-diming integration test in a private headless GNOME Shell.
#
#   dbus-run-session -- bash gnome-extension/tests/adaptive-shell.sh
#
# The extension is exercised against the real compositor: pattern actors are
# placed underneath its overlay, the extension captures the composited stage,
# and the resulting alpha is compared with the calibration table.  Only the
# pattern actors, the state document and the readings are synthetic; the
# capture, the histogram, the inversion and the overlay are the production
# code paths.
#
# Two virtual monitors are used so per-screen isolation can be checked without
# starting the Dimmer application.  The application is started at the end to
# verify the ReportAdaptiveMetrics / SetAdaptive round trip.
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIMMER_BIN=${DIMMER_BIN:-$ROOT/target/debug/dimmer}
EXTENSION_BUNDLE=${EXTENSION_BUNDLE:-}
BASE=${BASE:-/tmp/opencode/adaptive-shell}
LOG=$BASE/log
rm -rf "$BASE"
mkdir -p "$BASE"/{data,config,cache,runtime,dimmer-config,log,home}
chmod 700 "$BASE/runtime"
mkdir -p "$BASE/data/gnome-shell/extensions"

if [ -n "$EXTENSION_BUNDLE" ]; then
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
  "ShowIds": false,
  "ShowBoundaries": false,
  "NonDimmableWindow": false,
  "StartMinimised": false,
  "Master": { "On": false, "Dim": 30.0 },
  "Screens": [ { "Id": 1, "On": true, "Dim": 10.0, "Tint": "000000" } ]
}
JSON

export XDG_DATA_HOME=$BASE/data
export XDG_CONFIG_HOME=$BASE/config
export XDG_CACHE_HOME=$BASE/cache
export XDG_RUNTIME_DIR=$BASE/runtime
export WAYLAND_DISPLAY=wayland-dimmer
export LIBGL_ALWAYS_SOFTWARE=1
export MUTTER_DEBUG_DUMMY_MODE_SPECS=1920x1080
export HOME=$BASE/home

SHELL_PID=
DIMMER_PID=
FAILURES=0

cleanup() {
    [ -n "${DIMMER_PID:-}" ] && kill "$DIMMER_PID" 2>/dev/null
    [ -n "${SHELL_PID:-}" ] && kill "$SHELL_PID" 2>/dev/null
    wait 2>/dev/null
}
trap cleanup EXIT

fail() {
    FAILURES=$((FAILURES + 1))
    echo "FAIL: $*" >&2
}

pass() {
    echo "PASS: $*"
}

# --- shell helpers --------------------------------------------------------

shell_eval() {
    gdbus call --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Eval "$1" 2>&1
}

# Evaluate JS and return its string result.  Values must be scalars (numbers,
# booleans or text without quotes); anything else is a test bug.
scalar() {
    local out
    out=$(shell_eval "$1")
    out=${out#\(}
    out=${out%\)}
    case $out in
        true,\ *)
            out=${out#true, }
            out=${out#\'}
            out=${out%\'}
            ;;
        *)
            echo "eval failed: $out" >&2
            return 1
            ;;
    esac
    # The Shell JSON-encodes the value it returns, so a string result arrives
    # wrapped in double quotes with any embedded quotes escaped.
    out=$(printf '%s' "$out" | sed -e 's/^"//' -e 's/"$//' -e 's/\\"/"/g')
    printf '%s' "$out"
}

near() { # actual expected tolerance label
    if awk -v a="$1" -v b="$2" -v t="$3" \
        'BEGIN { d = a - b; if (d < 0) d = -d; exit (d <= t) ? 0 : 1 }'; then
        pass "$4 (got $1, expected $2 +/- $3)"
    else
        fail "$4 (got $1, expected $2 +/- $3)"
    fi
}

equal() { # actual expected label
    if [ "$1" = "$2" ]; then
        pass "$3 (got $1)"
    else
        fail "$3 (got $1, expected $2)"
    fi
}

truthy() { # value label
    equal "$1" "true" "$2"
}

# --- start the private session -------------------------------------------

gnome-shell --headless --unsafe-mode \
    --virtual-monitor 1920x1080 --virtual-monitor 1280x720 \
    --wayland-display=wayland-dimmer >"$LOG/shell.log" 2>&1 &
SHELL_PID=$!
started=0
for _ in $(seq 1 120); do
    if gdbus introspect --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell >/dev/null 2>&1; then
        started=1
        break
    fi
    sleep 0.1
done
if [ "$started" -ne 1 ]; then
    echo "shell did not start" >&2
    tail -40 "$LOG/shell.log" >&2
    exit 1
fi

for _ in $(seq 1 40); do
    gnome-extensions enable dimmer@dimmer.linux >/dev/null 2>&1 || true
    if gnome-extensions info dimmer@dimmer.linux 2>/dev/null |
            grep -q 'State: ACTIVE'; then
        break
    fi
    sleep 0.25
done

# A fresh headless session opens the overview and the welcome/end-session
# dialogs; both would legitimately pause adaptive mode.  Remove them so the
# measurement phases run against the same situation as a normal desktop.
if ! scalar '(function () {
    Main.overview.hide();
    Main.layoutManager.modalDialogGroup.get_children().forEach(c => c.destroy());
    return "clean";
})()' >/dev/null; then
    echo "could not prepare the Shell session" >&2
    exit 1
fi
sleep 1

monitor_count=$(scalar 'Main.layoutManager.monitors.length')
if [ "${monitor_count:-0}" -lt 2 ]; then
    echo "FAIL: expected two virtual monitors, got ${monitor_count:-0}" >&2
    exit 1
fi
pass "two virtual monitors available"

# --- install the test harness inside the Shell ---------------------------

if ! scalar '(function () {
    const St = imports.gi.St;
    const lookup = () => Main.extensionManager.lookup("dimmer@dimmer.linux")?.stateObj;
    const root = () => lookup()?._overlays?._root ?? null;
    const actors = new Map();

    function actor(name) {
        let a = actors.get(name);
        if (a)
            return a;
        a = new St.Widget({name, reactive: false, can_focus: false, track_hover: false});
        actors.set(name, a);
        global.window_group.add_child(a);
        return a;
    }

    function belowOverlay(a) {
        const overlay = root();
        if (overlay)
            global.window_group.set_child_below_sibling(a, overlay);
    }

    function paint(name, rect, css) {
        const a = actor(name);
        belowOverlay(a);
        a.set_position(rect.x, rect.y);
        a.set_size(rect.width, rect.height);
        a.set_style(`background-color: ${css};`);
        a.visible = true;
    }

    function monitor(index) {
        const m = Main.layoutManager.monitors[index];
        return {x: m.x, y: m.y, width: m.width, height: m.height};
    }

    function geometry(id) {
        return monitor(id - 1);
    }

    global.__dt = {
        paintFill(css) {
            const [width, height] = global.stage.get_size();
            paint("dimmer-test-fill", {x: 0, y: 0, width, height}, css);
        },
        paintMonitor(id, css) {
            paint(`dimmer-test-screen-${id}`, monitor(id - 1), css);
        },
        band(x, y, width, height, css) {
            let b = actors.get("dimmer-test-band");
            if (!b) {
                b = new St.Widget({name: "dimmer-test-band", reactive: false});
                actors.set("dimmer-test-band", b);
                Main.layoutManager.uiGroup.add_child(b);
            }
            b.set_position(x, y);
            b.set_size(width, height);
            b.set_style(`background-color: ${css};`);
            b.visible = true;
        },
        hideBand() {
            const b = actors.get("dimmer-test-band");
            if (b)
                b.visible = false;
        },
        panel() {
            const [x, y] = Main.panel.get_transformed_position();
            const [width, height] = Main.panel.get_transformed_size();
            return [x, y, width, height].join(" ");
        },
        configure(idsJson, dimsJson, strength) {
            Main.overview.hide();
            Main.layoutManager.modalDialogGroup.get_children().forEach(c => c.destroy());
            const ids = JSON.parse(idsJson);
            const dims = JSON.parse(dimsJson);
            const ext = lookup();
            const screens = ids.map((id, index) => ({
                id,
                on: true,
                dim: dims[index],
                tint: "000000",
                connector: `test-${index}`,
                geometry: geometry(id),
            }));

            ext._overlays.setState({
                screens,
                show_ids: false,
                show_boundaries: false,
                non_dimmable_window: false,
                window_visible: false,
                window_app_id: "",
                window_wm_class: "",
            });
            ext._adaptive.setState({
                adaptive: {on: true, strength},
                screens: screens.map(screen => ({
                    id: screen.id,
                    on: true,
                    dim: screen.dim,
                    geometry: screen.geometry,
                })),
            });
            return "ok";
        },
        read() {
            const ext = lookup();
            const controller = ext._adaptive;
            const rows = [...controller._screens.values()].map(entry => [
                entry.id,
                entry.alpha,
                entry.target,
                entry.metrics?.high ?? -1,
                entry.metrics?.mean ?? -1,
            ].join(":"));
            rows.push(`paused=${controller._paused}`);
            rows.push(`painted=${ext._overlays.isPainted()}`);
            rows.push(`givenUp=${controller._givenUp}`);
            rows.push(`available=${controller._available}`);
            rows.push(`sampling=${controller._sampling}`);
            rows.push(`failures=${controller._failures}`);
            rows.push(`lastSample=${controller._lastSample}`);
            rows.push(`now=${imports.gi.GLib.get_monotonic_time() / 1e6}`);
            rows.push(`modal=${Main.modalCount}`);
            rows.push(`overview=${Main.overview.visible}`);
            rows.push(`menu=${Boolean(Main.panel.menuManager.activeMenu)}`);
            rows.push(`locked=${Main.sessionMode.isLocked}`);
            return rows.join("|");
        },
        giveUp() {
            // The capture layer resolves the method at call time, so shadowing
            // it on the instance makes every following sample throw.
            try {
                global.stage.paint_to_content = () => {
                    throw new Error("injected capture failure");
                };
            } catch (error) {
                return `unsupported: ${error}`;
            }
            return "ok";
        },
        restoreCapture() {
            try {
                delete global.stage.paint_to_content;
            } catch (error) {
                return `unsupported: ${error}`;
            }
            return "ok";
        },
    };
    return "ready";
})()' >/dev/null; then
    echo "FAIL: could not install the adaptive test harness" >&2
    exit 1
fi
pass "test harness installed"

# Field extraction: `field <screen-id> <column>` from the last `read`.
LAST_READ=""
read_state() {
    LAST_READ=$(scalar 'global.__dt.read()') || return 1
}
screen_field() { # id column (2=alpha, 3=target, 4=high, 5=mean)
    printf '%s' "$LAST_READ" | tr '|' '\n' |
        awk -F: -v id="$1" -v col="$2" '$1 == id { print $col; exit }'
}
global_field() { # paused|painted|givenUp|available
    printf '%s' "$LAST_READ" | tr '|' '\n' |
        awk -F= -v key="$1" '$1 == key { print $2; exit }'
}

wait_for() { # seconds label
    sleep "$1"
    read_state || fail "could not read controller state for $2"
}

# --- calibration phases (one monitor, 10 % user dim, strength 50) --------

if ! scalar "global.__dt.configure('[1]', '[10]', 50)" >/dev/null; then
    echo "FAIL: could not publish a synthetic state" >&2
    exit 1
fi

# Full white: 90 % after the user's dim, above both targets, so the result is
# the adaptive cap.  0.50 is also the headroom at 10 % user dim.
scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
wait_for 2.0 "white"

alpha=$(screen_field 1 2)
high=$(screen_field 1 4)
near "${high:-0}" 0.90 0.02 "white content measured at 90% after the 10% dim"
near "${alpha:-0}" 0.50 0.015 "full white reaches the adaptive cap"
truthy "$(global_field painted)" "overlay is painted during measurement"

# Uniform mid grey: the average limiter drives the result to ~0.451.
scalar 'global.__dt.paintFill("#b4b4b4")' >/dev/null
wait_for 2.5 "grey 180"
alpha=$(screen_field 1 2)
near "${alpha:-0}" 0.451 0.02 "uniform 71% grey settles at the average limiter"

# Uniform 50% grey: the target is ~0.222. Release is slow, so give it time.
scalar 'global.__dt.paintFill("#808080")' >/dev/null
wait_for 3.5 "grey 128"
alpha=$(screen_field 1 2)
target=$(screen_field 1 3)
near "${alpha:-0}" 0.222 0.02 "uniform 50% grey settles at the average limiter"
near "${alpha:-0}" "${target:-0}" 0.01 "alpha has settled onto its target"

# Strength 100 moves the targets down, so the same grey must dim further.
scalar "global.__dt.configure('[1]', '[10]', 100)" >/dev/null
wait_for 1.5 "strength 100"
alpha=$(screen_field 1 2)
near "${alpha:-0}" 0.380 0.02 "strength 100 darkens the same content further"

# --- release ------------------------------------------------------------

scalar 'global.__dt.paintFill("#000000")' >/dev/null
wait_for 1.2 "black after white"
alpha=$(screen_field 1 2)
if awk -v a="${alpha:-0}" 'BEGIN { exit (a > 0.15 && a < 0.34) ? 0 : 1 }'; then
    pass "release is gradual (alpha $alpha after 1.2 s)"
else
    fail "release should be gradual, got alpha ${alpha:-missing}"
fi

wait_for 5.0 "black settled"
alpha=$(screen_field 1 2)
target=$(screen_field 1 3)
equal "$target" "0" "black content targets zero"
near "${alpha:-1}" 0 0.001 "alpha reaches zero on black content"

# --- panel masking ------------------------------------------------------

# A white band exactly where the panel is must not count: the panel is never
# dimmed, so it is excluded from every capture.  Starting from white makes the
# test detect a missing exclusion immediately.
scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
wait_for 2.0 "white before panel test"
alpha=$(screen_field 1 2)
near "${alpha:-0}" 0.50 0.015 "white restores the cap before the panel test"

read -r panel_x panel_y panel_w panel_h <<EOF
$(scalar 'global.__dt.panel()')
EOF
scalar 'global.__dt.paintFill("#000000")' >/dev/null
scalar "global.__dt.band($panel_x, $panel_y, $panel_w, $panel_h, '#ffffff')" >/dev/null
wait_for 1.5 "panel band"
target=$(screen_field 1 3)
high=$(screen_field 1 4)
equal "$target" "0" "white in the panel band is excluded from the target"
near "${high:-1}" 0 0.02 "the panel band does not contribute brightness"
scalar 'global.__dt.hideBand()' >/dev/null

# --- headroom -----------------------------------------------------------

scalar "global.__dt.configure('[1]', '[90]', 50)" >/dev/null
scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
wait_for 2.0 "90% user dim"
alpha=$(screen_field 1 2)
near "${alpha:-1}" 0 0.001 "no adaptive darkness is added at 90% user dim"

# --- overview pauses without releasing ---------------------------------

scalar "global.__dt.configure('[1]', '[10]', 50)" >/dev/null
scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
wait_for 2.0 "white before overview"
alpha=$(screen_field 1 2)
near "${alpha:-0}" 0.50 0.015 "alpha is at the cap before the overview"

scalar 'Main.overview.show(); "shown"' >/dev/null
wait_for 1.5 "overview shown"
alpha=$(screen_field 1 2)
truthy "$(global_field paused)" "adaptive pauses while the overview is open"
near "${alpha:-0}" 0.50 0.001 "alpha is frozen, not released, behind the overview"
equal "$(global_field painted)" "false" "the overlay is hidden behind the overview"

scalar 'Main.overview.hide(); "hidden"' >/dev/null
wait_for 1.5 "overview hidden"
alpha=$(screen_field 1 2)
equal "$(global_field paused)" "false" "adaptive resumes after the overview"
near "${alpha:-0}" 0.50 0.015 "alpha is unchanged after resuming"

# --- two monitors are independent --------------------------------------

# Black everywhere, white only on monitor 1.  Monitor 2 must stay at zero.
scalar "global.__dt.configure('[1,2]', '[10,10]', 50)" >/dev/null
scalar 'global.__dt.paintFill("#000000")' >/dev/null
scalar 'global.__dt.paintMonitor(1, "#ffffff")' >/dev/null
wait_for 3.0 "two monitors"

alpha1=$(screen_field 1 2)
alpha2=$(screen_field 2 2)
near "${alpha1:-0}" 0.50 0.015 "bright monitor gets the adaptive layer"
near "${alpha2:-1}" 0 0.001 "dark monitor stays untouched"

# --- repeated capture failure gives up cleanly -------------------------

scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
wait_for 2.0 "white before failure injection"
alpha=$(screen_field 1 2)
if awk -v a="${alpha:-0}" 'BEGIN { exit (a > 0.01) ? 0 : 1 }'; then
    pass "adaptive is active before the failure injection (alpha $alpha)"
else
    fail "adaptive should be active before the failure injection (alpha ${alpha:-missing})"
fi

if ! scalar 'global.__dt.giveUp()' | grep -q '^ok$'; then
    echo "SKIP: this Shell does not allow shadowing paint_to_content" >&2
else
    wait_for 2.0 "capture failure"
    truthy "$(global_field givenUp)" "stops sampling after repeated capture failures"
    alpha=$(screen_field 1 2)
    near "${alpha:-1}" 0 0.001 "gives up without leaving darkness behind"
    equal "$(global_field available)" "false" "reports adaptive as unavailable"

    rects=$(scalar '(() => {
        const ext = Main.extensionManager.lookup("dimmer@dimmer.linux").stateObj;
        const actors = ext._overlays._screenActors;
        let visible = 0;
        for (const entry of actors.values())
            for (const rect of entry.adaptiveRects)
                if (rect.visible)
                    visible++;
        return visible;
    })()')
    equal "${rects:-?}" "0" "all adaptive actors are hidden after giving up"
    scalar 'global.__dt.restoreCapture()' >/dev/null
fi

# --- disable / re-enable is clean --------------------------------------

gnome-extensions disable dimmer@dimmer.linux >/dev/null 2>&1 || true
sleep 1
leftovers=$(scalar '(() => {
    const root = global.window_group.get_children().find(
        actor => actor && actor.name === "dimmer-overlay-root");
    const indicator = Boolean(Main.panel.statusArea["dimmer@dimmer.linux"]);
    return `${Boolean(root)} ${indicator}`;
})()')
equal "$leftovers" "false false" "disabling removes the overlay and the indicator"

for _ in $(seq 1 40); do
    gnome-extensions enable dimmer@dimmer.linux >/dev/null 2>&1 || true
    if gnome-extensions info dimmer@dimmer.linux 2>/dev/null |
            grep -q 'State: ACTIVE'; then
        break
    fi
    sleep 0.25
done
sleep 1

# The helper object belongs to the old extension instance, so reinstall it and
# verify the fresh controller still reaches the same calibrated alpha.
if ! scalar '(function () {
    const St = imports.gi.St;
    const lookup = () => Main.extensionManager.lookup("dimmer@dimmer.linux")?.stateObj;
    const root = () => lookup()?._overlays?._root ?? null;
    global.__dt = {
        configure(idsJson, dimsJson, strength) {
            Main.overview.hide();
            Main.layoutManager.modalDialogGroup.get_children().forEach(c => c.destroy());
            const ids = JSON.parse(idsJson);
            const dims = JSON.parse(dimsJson);
            const ext = lookup();
            const screens = ids.map((id, index) => {
                const m = Main.layoutManager.monitors[id - 1];
                return {
                    id,
                    on: true,
                    dim: dims[index],
                    tint: "000000",
                    connector: `test-${index}`,
                    geometry: {x: m.x, y: m.y, width: m.width, height: m.height},
                };
            });
            ext._overlays.setState({
                screens,
                show_ids: false,
                show_boundaries: false,
                non_dimmable_window: false,
                window_visible: false,
                window_app_id: "",
                window_wm_class: "",
            });
            ext._adaptive.setState({
                adaptive: {on: true, strength},
                screens: screens.map(screen => ({
                    id: screen.id,
                    on: true,
                    dim: screen.dim,
                    geometry: screen.geometry,
                })),
            });
            return "ok";
        },
        paintFill(css) {
            const [width, height] = global.stage.get_size();
            let a = global.window_group.get_children().find(
                actor => actor && actor.name === "dimmer-test-fill");
            if (!a) {
                a = new St.Widget({name: "dimmer-test-fill", reactive: false});
                global.window_group.add_child(a);
            }
            const overlay = root();
            if (overlay)
                global.window_group.set_child_below_sibling(a, overlay);
            a.set_position(0, 0);
            a.set_size(width, height);
            a.set_style(`background-color: ${css};`);
            a.visible = true;
        },
        read() {
            const ext = lookup();
            const controller = ext._adaptive;
            const entry = [...controller._screens.values()][0];
            return entry
                ? `${entry.alpha} ${entry.target} ${controller._paused}`
                : "missing";
        },
    };
    return "ready";
})()' >/dev/null; then
    fail "could not reinstall the harness after re-enabling"
else
    scalar "global.__dt.configure('[1]', '[10]', 50)" >/dev/null
    scalar 'global.__dt.paintFill("#ffffff")' >/dev/null
    sleep 2
    reading=$(scalar 'global.__dt.read()')
    alpha=$(printf '%s' "$reading" | cut -d' ' -f1)
    near "${alpha:-0}" 0.50 0.015 "a re-enabled extension reaches the same alpha"
fi

# --- application round trip --------------------------------------------

XDG_CONFIG_HOME=$BASE/dimmer-config \
    WAYLAND_DISPLAY=wayland-dimmer \
    "$DIMMER_BIN" --show >"$LOG/dimmer.log" 2>&1 &
DIMMER_PID=$!

connected=0
for _ in $(seq 1 100); do
    if gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>/dev/null |
            grep -q '"extension_connected":true'; then
        connected=1
        break
    fi
    sleep 0.2
done
if [ "$connected" -ne 1 ]; then
    fail "Dimmer did not start and register the extension"
else
    pass "Dimmer application registered the extension"

    scalar "global.__dt.configure('[1]', '[10]', 50)" >/dev/null
    scalar 'global.__dt.paintFill("#b4b4b4")' >/dev/null
    sleep 3

    metrics=$(gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetAdaptiveMetrics 2>&1)
    case $metrics in
        *'"available":true'*) pass "metrics report availability" ;;
        *) fail "metrics payload should report availability: $metrics" ;;
    esac
    case $metrics in
        *'"id":1'*) pass "metrics name the measured screen" ;;
        *) fail "metrics payload should include screen 1: $metrics" ;;
    esac

    alpha=$(printf '%s' "$metrics" |
        sed -n 's/.*"alpha":\([0-9.]*\).*/\1/p' | head -1)
    near "${alpha:-0}" 0.451 0.02 "reported alpha matches the measured content"

    gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.SetAdaptive true 70 >/dev/null 2>&1
    sleep 0.5
    state=$(gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>&1)
    case $state in
        *'"adaptive":{"on":true,"strength":70'*) pass "SetAdaptive publishes the new settings" ;;
        *) fail "state should carry adaptive on/70: $state" ;;
    esac

    if grep -q '"Adaptive"' "$BASE/dimmer-config/dimmer/Dimmer.ini" 2>/dev/null; then
        pass "adaptive settings are persisted"
    else
        fail "adaptive settings were not written to Dimmer.ini"
    fi

    gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.SetAdaptive false 50 >/dev/null 2>&1
    sleep 0.3
    state=$(gdbus call --session --dest io.github.dimmer.Linux \
        --object-path /io/github/dimmer/Linux \
        --method io.github.dimmer.Linux.GetState 2>&1)
    case $state in
        *'"adaptive":{"on":false'*) pass "SetAdaptive can switch the feature off" ;;
        *) fail "state should carry adaptive off: $state" ;;
    esac
fi

# --- result -------------------------------------------------------------

if grep -qiE 'JS ERROR|Unhandled promise rejection' "$LOG/shell.log"; then
    fail "the Shell logged JavaScript errors"
    grep -iE -A 5 'JS ERROR|Unhandled promise rejection' "$LOG/shell.log" | tail -30 >&2
else
    pass "no JavaScript errors in the Shell log"
fi

if [ "$FAILURES" -ne 0 ]; then
    echo "adaptive-shell: $FAILURES failure(s)" >&2
    exit 1
fi

echo "adaptive-shell: all checks passed"
