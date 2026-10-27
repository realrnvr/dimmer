// GNOME Shell window reset tests. Run with: gjs -m gnome-extension/tests/window-reset.test.js

import {
    centeredWindowFrame,
    resetWindowFrame,
    windowMatchesResetFrame,
} from '../dimmer@dimmer.linux/windowReset.js';

let failures = 0;

function equal(actual, expected, label) {
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
        failures++;
        printerr(`FAIL: ${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

function ok(value, label) {
    if (!value) {
        failures++;
        printerr(`FAIL: ${label}`);
    }
}

equal(
    centeredWindowFrame({x: 0, y: 32, width: 1920, height: 1048}, 1040, 700),
    {x: 440, y: 206, width: 1040, height: 700},
    'centers default size inside primary work area');

equal(
    centeredWindowFrame({x: -1280, y: 24, width: 800, height: 600}, 1040, 700),
    {x: -1280, y: 24, width: 800, height: 600},
    'clamps window to smaller work area');

equal(centeredWindowFrame({x: 0, y: 0, width: 0, height: 700}, 1040, 700), null,
    'rejects empty work area');

const calls = [];
const window = {
    maximized_horizontally: false,
    maximized_vertically: false,
    unmaximize: () => calls.push(['unmaximize']),
    move_resize_frame: (...args) => calls.push(['move_resize_frame', ...args]),
    get_frame_rect: () => ({x: 440, y: 206, width: 1040, height: 700}),
};

const target = resetWindowFrame(window,
    {x: 0, y: 32, width: 1920, height: 1048}, 1040, 700);
equal(target, {x: 440, y: 206, width: 1040, height: 700},
    'returns the requested frame');
equal(calls, [['move_resize_frame', false, 440, 206, 1040, 700]],
    'uses one Mutter geometry request when not maximized');
ok(windowMatchesResetFrame(window, target), 'confirms matching geometry');
ok(!windowMatchesResetFrame(window,
    {x: 500, y: 206, width: 1040, height: 700}), 'rejects mismatched geometry');

calls.length = 0;
window.maximized_horizontally = true;
const maximizedTarget = resetWindowFrame(window,
    {x: 0, y: 32, width: 1920, height: 1048}, 1040, 700);
equal(calls, [
    ['unmaximize'],
    ['move_resize_frame', false, 440, 206, 1040, 700],
], 'unmaximizes only when needed and issues one geometry request');
ok(!windowMatchesResetFrame(window, maximizedTarget),
    'does not confirm success while maximized');

equal(resetWindowFrame(window, null, 1040, 700), null,
    'does not reset without a work area');

if (failures > 0)
    throw new Error(`${failures} window reset test(s) failed`);

print('PASS: window reset geometry and Mutter operations');
