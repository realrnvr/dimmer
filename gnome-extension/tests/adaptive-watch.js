// Watch adaptive-diming metrics from a running Dimmer.
//
//   gjs -m gnome-extension/tests/adaptive-watch.js
//   gjs -m gnome-extension/tests/adaptive-watch.js --interval=250 --csv=/tmp/adaptive.csv
//
// Prints one line per sample with the measured peak/average, the alpha the
// extension says it applies, and the alpha implied by the same control law.
// Agreement means capture and control are fine; a persistent difference is a
// controller bug. It does not prove the layer rendered — use the screenshot
// check in docs/TEST_PLAN.md for that.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GLibUnix from 'gi://GLibUnix';
import System from 'system';

const BUS = 'io.github.dimmer.Linux';
const PATH = '/io/github/dimmer/Linux';
const IFACE = 'io.github.dimmer.Linux';
const MISMATCH = 0.02;

const clamp = (value, low, high) => Math.min(high, Math.max(low, value));

function usage() {
    print(`Usage: gjs -m ${System.programInvocationName} [OPTIONS]

Polls io.github.dimmer.Linux.GetAdaptiveMetrics and prints a table.

Options:
  --interval=MS    Sampling interval in milliseconds (default 500)
  --duration=SEC   Stop after this many seconds (default: run until Ctrl-C)
  --csv=FILE       Write raw samples to FILE when the watcher stops
  --help, -h       Show this help

Baseline for comparable numbers: one monitor, screen dim 10%, adaptive on,
strength 50. Run the adaptive-flash.html page fullscreen on that monitor.`);
}

let intervalMs = 500;
let durationS = 0;
let csvPath = null;

for (const argument of ARGV) {
    if (argument === '--help' || argument === '-h') {
        usage();
        System.exit(0);
    } else if (argument.startsWith('--interval=')) {
        intervalMs = Math.max(100, Number(argument.slice('--interval='.length)) || 500);
    } else if (argument.startsWith('--duration=')) {
        durationS = Math.max(0, Number(argument.slice('--duration='.length)) || 0);
    } else if (argument.startsWith('--csv=')) {
        csvPath = argument.slice('--csv='.length);
    } else {
        printerr(`unknown argument: ${argument}\n`);
        usage();
        System.exit(2);
    }
}

const connection = Gio.bus_get_sync(Gio.BusType.SESSION, null);

function call(method, parameters = null) {
    const reply = connection.call_sync(
        BUS, PATH, IFACE, method, parameters, null,
        Gio.DBusCallFlags.NONE, 3000, null);
    return reply.deep_unpack()[0];
}

function impliedAlpha(high, mean, strength, dim) {
    if (high === null || high === undefined || mean === null || mean === undefined)
        return null;
    const s = clamp(strength, 0, 100) / 100;
    const highTarget = 0.72 - 0.20 * s;
    const meanTarget = 0.42 - 0.14 * s;
    const headroom = Math.max(0, Math.min(0.50, 0.90 - clamp(dim, 0, 90) / 100));
    if (!(high > 0) || !(mean > 0) || headroom <= 0)
        return 0;
    const needHigh = high > highTarget ? 1 - highTarget / high : 0;
    const needMean = mean > meanTarget ? 1 - meanTarget / mean : 0;
    return clamp(Math.max(needHigh, needMean), 0, headroom);
}

function percent(value) {
    if (value === null || value === undefined)
        return '   -  ';
    return `${(value * 100).toFixed(1).padStart(5)}%`;
}

function millis(value) {
    if (value === null || value === undefined)
        return '    -';
    return value.toFixed(1).padStart(5);
}

const loop = new GLib.MainLoop(null, false);
const started = GLib.get_monotonic_time() / 1e6;
let timeoutId = 0;
let finished = false;
let samples = 0;
let mismatches = 0;
let strength = 50;
let dims = new Map();
let toldMetricsMissing = false;
let toldNoScreens = false;
let toldAdaptiveOff = false;
const streaks = new Map();
const csv = ['time,screen,dim,high,mean,alpha,implied,paused,available,sample_ms'];

function finish() {
    if (finished)
        return;
    finished = true;
    if (timeoutId !== 0) {
        GLib.source_remove(timeoutId);
        timeoutId = 0;
    }

    const elapsed = GLib.get_monotonic_time() / 1e6 - started;
    print(`\n# ${samples} samples in ${elapsed.toFixed(1)} s, ` +
        `${mismatches} persistent mismatch(es)`);

    if (csvPath) {
        if (GLib.file_set_contents(csvPath, `${csv.join('\n')}\n`))
            print(`# wrote ${csvPath}`);
        else
            printerr(`could not write ${csvPath}`);
    }

    loop.quit();
}

function readState() {
    const state = JSON.parse(call('GetState'));
    const adaptive = state.adaptive ?? {on: false, strength: 50};
    strength = Number(adaptive.strength) || 50;
    dims = new Map((state.screens ?? []).map(screen => [
        screen.id, Number(screen.dim) || 0,
    ]));

    if (!adaptive.on && !toldAdaptiveOff) {
        toldAdaptiveOff = true;
        print('# adaptive is off in Dimmer; enable Options -> Adaptive');
    }
}

function poll() {
    let raw;
    try {
        readState();
        raw = call('GetAdaptiveMetrics');
    } catch (error) {
        const message = error?.message ?? String(error);
        printerr(`# ${message}`);
        if (message.includes('UnknownMethod') || message.includes('Unknown method')) {
            printerr('# this Dimmer predates adaptive metrics; reinstall 1.2.0 or newer');
            finish();
        }
        return;
    }

    const metrics = raw === 'null' ? null : JSON.parse(raw);
    const elapsed = GLib.get_monotonic_time() / 1e6 - started;

    if (metrics === null) {
        if (!toldMetricsMissing) {
            toldMetricsMissing = true;
            print('# no metrics yet: the extension has not reported since it connected');
        }
        return;
    }
    toldMetricsMissing = false;

    const screens = metrics.screens ?? [];
    if (screens.length === 0) {
        if (!toldNoScreens) {
            toldNoScreens = true;
            print('# no eligible screens: adaptive needs a screen that is on and dim > 0');
        }
        return;
    }
    toldNoScreens = false;

    for (const screen of screens) {
        const dim = dims.get(screen.id) ?? 0;
        const implied = impliedAlpha(screen.high, screen.mean, strength, dim);
        const flags = [];
        if (metrics.paused)
            flags.push('paused');
        if (!metrics.available)
            flags.push('unavailable');

        if (!metrics.paused && implied !== null) {
            const delta = Math.abs(screen.alpha - implied);
            const streak = delta > MISMATCH ? (streaks.get(screen.id) ?? 0) + 1 : 0;
            streaks.set(screen.id, streak);
            if (streak >= 3) {
                flags.push('MISMATCH');
                if (streak === 3)
                    mismatches++;
            }
        } else {
            streaks.set(screen.id, 0);
        }

        print([
            `${elapsed.toFixed(1).padStart(6)}s`,
            String(screen.id).padStart(3),
            `${dim.toFixed(0).padStart(3)}%`,
            percent(screen.high),
            percent(screen.mean),
            percent(screen.alpha),
            percent(implied),
            millis(screen.sample_ms),
            flags.join(',') || '-',
        ].join('  '));

        csv.push([
            elapsed.toFixed(2),
            screen.id,
            dim,
            screen.high ?? '',
            screen.mean ?? '',
            screen.alpha,
            implied ?? '',
            metrics.paused,
            metrics.available,
            screen.sample_ms ?? '',
        ].join(','));
        samples++;
    }
}

try {
    readState();
} catch (error) {
    printerr(`Dimmer is not responding on the session bus: ${error?.message ?? error}`);
    printerr('Start it with "dimmer --show", then enable adaptive dimming.');
    System.exit(1);
}

print('# Dimmer adaptive watcher — Ctrl-C to stop');
print('#  time  scr  dim    peak    mean   alpha   imply     ms  flags');
poll();

timeoutId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, intervalMs, () => {
    if (durationS > 0 && GLib.get_monotonic_time() / 1e6 - started >= durationS) {
        finish();
        return GLib.SOURCE_REMOVE;
    }
    poll();
    return GLib.SOURCE_CONTINUE;
});

GLibUnix.signal_add(GLib.PRIORITY_DEFAULT, 2, () => {
    finish();
    return GLib.SOURCE_REMOVE;
});
GLibUnix.signal_add(GLib.PRIORITY_DEFAULT, 15, () => {
    finish();
    return GLib.SOURCE_REMOVE;
});

loop.run();
