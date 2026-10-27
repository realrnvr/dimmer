// Integration smoke test for the extension's D-Bus client.
//
// Requires a running Dimmer instance on the same session bus:
//   XDG_CONFIG_HOME=... ./target/debug/dimmer --show
// Then:
//   gjs -m gnome-extension/tests/dbus-client.test.js

import GLib from 'gi://GLib';

import {DimmerDbusClient} from '../dimmer@dimmer.linux/dbusClient.js';

const TIMEOUT_SECONDS = 10;

let client = null;
let loop = null;
let timeoutId = 0;
let originalMaster = null;
let screenCount = 0;
let expectedRestoreOn = false;
let phase = 'connect';
let failures = 0;

function fail(message) {
    failures += 1;
    printerr(`FAIL: ${message}`);
}

function finish() {
    if (timeoutId !== 0) {
        GLib.source_remove(timeoutId);
        timeoutId = 0;
    }
    if (client) {
        client.extensionDisconnected();
        client.destroy();
        client = null;
    }
    if (loop) {
        loop.quit();
        loop = null;
    }
}

function check(condition, message) {
    if (!condition)
        fail(message);
}

function onState(_client, state) {
    check(state.protocol === 1, 'state protocol is 1');
    check(Array.isArray(state.screens), 'state has screens array');
    check(state.master && typeof state.master === 'object', 'state has master object');
    check(typeof state.window_visible === 'boolean', 'state has window_visible');

    if (phase === 'connect') {
        screenCount = state.screens.length;
        originalMaster = {
            on: Boolean(state.master.on),
            dim: Number(state.master.dim) || 0,
        };
        // Master control is only valid with more than one monitor.
        expectedRestoreOn = screenCount > 1 ? originalMaster.on : false;
        print(`connected: ${screenCount} screen(s), master=${originalMaster.dim}%`);
        phase = screenCount > 1 ? 'set-master' : 'set-master-single';
        client.setMaster(true, 33.5);
        return;
    }

    if (phase === 'set-master') {
        if (Math.abs(Number(state.master.dim) - 33.5) < 0.01 && state.master.on === true) {
            print('SetMaster round-trip verified');
            phase = 'restore';
            client.setMaster(originalMaster.on, originalMaster.dim);
        }
        return;
    }

    if (phase === 'set-master-single') {
        if (Math.abs(Number(state.master.dim) - 33.5) < 0.01 &&
            state.master.on === false) {
            print('SetMaster correctly rejected with one screen');
            phase = 'restore';
            client.setMaster(originalMaster.on, originalMaster.dim);
        }
        return;
    }

    if (phase === 'restore') {
        if (Math.abs(Number(state.master.dim) - originalMaster.dim) < 0.01 &&
            Boolean(state.master.on) === expectedRestoreOn) {
            print('master state restored');
            phase = 'done';
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 250, () => {
                finish();
                return GLib.SOURCE_REMOVE;
            });
        }
    }
}

loop = new GLib.MainLoop(null, false);
client = new DimmerDbusClient(1);

client.connect('connected', () => print('Dimmer service connected'));
client.connect('disconnected', () => {
    if (phase !== 'done')
        fail('service disconnected during test');
});
client.connect('state-changed', onState);

timeoutId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, TIMEOUT_SECONDS, () => {
    timeoutId = 0;
    fail(`timed out during phase ${phase}`);
    finish();
    return GLib.SOURCE_REMOVE;
});

client.enable();
loop.run();

if (failures > 0) {
    printerr(`dbus-client: ${failures} failure(s)`);
    imports.system.exit(1);
}
print('dbus-client: all checks passed');
