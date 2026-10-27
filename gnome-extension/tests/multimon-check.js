#!/usr/bin/env -S gjs -m
// Checks Dimmer's two-monitor behaviour against a running app and Shell.
//
// Invoked by multimon-shell.sh. Both sides are reached over the private
// session bus, so this script never touches the real desktop.

import Gio from "gi://Gio";
import GLib from "gi://GLib";
import System from "system";

const APP_BUS = "io.github.dimmer.Linux";
const APP_PATH = "/io/github/dimmer/Linux";
const APP_IFACE = "io.github.dimmer.Linux";
const SHELL_BUS = "org.gnome.Shell";
const SHELL_PATH = "/org/gnome/Shell";
const SHELL_IFACE = "org.gnome.Shell";

const problems = [];

function session() {
    return Gio.bus_get_sync(Gio.BusType.SESSION, null);
}

function callApp(conn, method, parameters = null, replyType = "(s)") {
    const reply = conn.call_sync(
        APP_BUS,
        APP_PATH,
        APP_IFACE,
        method,
        parameters,
        GLib.VariantType.new(replyType),
        Gio.DBusCallFlags.NONE,
        5000,
        null,
    );
    return reply.deepUnpack();
}

function state(conn) {
    return JSON.parse(callApp(conn, "GetState")[0]);
}

function actors(conn) {
    const expression = `
(() => {
  const root = global.window_group.get_children()
    .find(a => a.name === 'dimmer-overlay-root');
  if (!root)
    return JSON.stringify(null);
  return JSON.stringify(root.get_children().map(screen => {
    const child = suffix => screen.get_children()
      .find(c => c.name.startsWith(suffix));
    const rects = child('dimmer-overlay-rects-');
    const boundary = child('dimmer-overlay-boundary-');
    const label = child('dimmer-overlay-id-');
    return {
      name: screen.name,
      visible: screen.visible,
      label: label ? label.text : null,
      labelVisible: label ? label.visible : false,
      boundary: boundary ? {
        x: boundary.x, y: boundary.y,
        width: boundary.width, height: boundary.height,
        visible: boundary.visible,
      } : null,
      rects: rects ? rects.get_children().map(r => ({
        x: r.x, y: r.y, width: r.width, height: r.height,
        visible: r.visible, style: r.style,
      })) : [],
    };
  }));
})()
`;
    const reply = conn.call_sync(
        SHELL_BUS,
        SHELL_PATH,
        SHELL_IFACE,
        "Eval",
        new GLib.Variant("(s)", [expression]),
        GLib.VariantType.new("(bs)"),
        Gio.DBusCallFlags.NONE,
        5000,
        null,
    );
    const [ok, text] = reply.deepUnpack();
    if (!ok)
        throw new Error(`Shell Eval failed: ${text}`);
    let value = JSON.parse(text);
    // Eval marshals a returned string as JSON; JSON.stringify therefore
    // arrives one level deeper than GetState's plain string.
    if (typeof value === "string")
        value = JSON.parse(value);
    return value;
}

function check(condition, message) {
    if (!condition)
        problems.push(message);
}

function alpha(style) {
    // St style: "background-color: rgba(0, 0, 0, 0.550);"
    const match = style.match(
        /rgba\(\s*[\d.]+\s*,\s*[\d.]+\s*,\s*[\d.]+\s*,\s*([\d.]+)\s*\)/,
    );
    return match ? Number.parseFloat(match[1]) : null;
}

function visibleRects(screen) {
    return screen.rects.filter(rect => rect.visible);
}

// Polls until predicate is true; records a failure on timeout.
//
// D-Bus mutations are acknowledged before the app's event loop forwards the
// new state to the Shell extension, so actor assertions have to wait for the
// Shell side to settle instead of reading immediately after the call.
function waitUntil(predicate, description, timeout = 5.0) {
    const deadline = GLib.get_monotonic_time() + timeout * 1_000_000;
    while (GLib.get_monotonic_time() < deadline) {
        if (predicate())
            return true;
        GLib.usleep(50_000);
    }
    problems.push(`timed out waiting for ${description}`);
    return false;
}

function screenGroup(tree, screenId) {
    return tree.find(screen => screen.name.endsWith(`-${screenId}`));
}

function main() {
    const conn = session();
    const data = state(conn);
    const monitors = data.screens;
    check(monitors.length === 2, `expected 2 screens, got ${monitors.length}`);
    check(
        monitors[0].id === 1 && monitors[1].id === 2,
        `unexpected screen ids: ${monitors.map(m => m.id)}`,
    );
    check(
        monitors[0].connector !== monitors[1].connector,
        "both screens report the same connector",
    );
    const widths = monitors.map(m => m.geometry.width).sort((a, b) => a - b);
    check(
        widths.length === 2 && widths[0] === 1280 && widths[1] === 1920,
        `unexpected monitor widths: ${widths}`,
    );
    const xs = monitors.map(m => m.geometry.x).sort((a, b) => a - b);
    check(
        xs.length === 2 && xs[0] === 0 && xs[1] === 1920,
        `unexpected monitor x positions: ${monitors.map(m => m.geometry.x)}`,
    );
    print(
        "state: screens =",
        JSON.stringify(
            monitors.map(m => [m.id, m.connector, m.geometry, m.dim, m.on]),
        ),
    );

    const tree = actors(conn);
    check(
        Array.isArray(tree) && tree.length === 2,
        `expected 2 overlay screen groups, got ${JSON.stringify(tree)}`,
    );
    if (!Array.isArray(tree) || tree.length !== 2)
        return;
    check(
        JSON.stringify(tree.map(screen => screen.name).sort()) ===
            JSON.stringify([
                "dimmer-overlay-screen-1",
                "dimmer-overlay-screen-2",
            ]),
        `unexpected overlay names: ${tree.map(s => s.name)}`,
    );
    for (const screen of tree) {
        check(screen.visible, `${screen.name} is not visible`);
        check(
            screen.label === "1" || screen.label === "2",
            `bad label: ${JSON.stringify(screen.label)}`,
        );
        check(screen.labelVisible, `${screen.name} label is hidden`);
        check(
            screen.boundary && screen.boundary.visible,
            `${screen.name} boundary is hidden`,
        );
        check(
            visibleRects(screen).length === 1,
            `${screen.name} should have one full-monitor rectangle`,
        );
    }
    const byId = {
        1: tree.find(screen => screen.name.endsWith("-1")),
        2: tree.find(screen => screen.name.endsWith("-2")),
    };
    for (const screen of monitors) {
        const group = byId[screen.id];
        const boundary = group.boundary;
        check(
            boundary.x === screen.geometry.x &&
                boundary.y === screen.geometry.y &&
                boundary.width === screen.geometry.width &&
                boundary.height === screen.geometry.height,
            `boundary for screen ${screen.id} does not match geometry`,
        );
        const rect = visibleRects(group)[0];
        check(
            rect.x === screen.geometry.x &&
                rect.y === screen.geometry.y &&
                rect.width === screen.geometry.width &&
                rect.height === screen.geometry.height,
            `rectangle for screen ${screen.id} does not match geometry`,
        );
    }

    // State dims use the original 0-100 scale; overlay opacity is 0-1.
    const expectedDim = { 1: 40.0, 2: 60.0 };
    const expectedAlpha = { 1: 0.4, 2: 0.6 };
    for (const screen of monitors) {
        check(
            Math.abs(screen.dim - expectedDim[screen.id]) < 0.01,
            `screen ${screen.id} dim is ${screen.dim}, expected ${expectedDim[screen.id]}`,
        );
        const got = alpha(visibleRects(byId[screen.id])[0].style);
        check(
            got !== null && Math.abs(got - expectedAlpha[screen.id]) < 0.01,
            `screen ${screen.id} alpha is ${got}, expected ${expectedAlpha[screen.id]}`,
        );
    }

    callApp(conn, "SetScreen", new GLib.Variant("(ibd)", [2, false, 60.0]), "()");
    waitUntil(
        () => !state(conn).screens[1].on,
        "screen 2 state to turn off",
    );
    waitUntil(
        () => visibleRects(screenGroup(actors(conn), 2)).length === 0,
        "screen 2 rectangles to hide",
    );
    const fresh = actors(conn);
    check(
        !screenGroup(fresh, 2).rects.some(rect => rect.visible),
        "screen 2 rectangles stayed visible after turning it off",
    );
    check(
        screenGroup(fresh, 1).rects.some(rect => rect.visible),
        "screen 1 rectangles disappeared when screen 2 was turned off",
    );

    callApp(conn, "SetScreen", new GLib.Variant("(ibd)", [2, true, 60.0]), "()");
    callApp(conn, "SetMaster", new GLib.Variant("(bd)", [true, 55.0]), "()");
    waitUntil(() => state(conn).master.on, "master control to turn on");
    waitUntil(
        () => state(conn).screens.every(screen => Math.abs(screen.dim - 55.0) < 0.01),
        "master dim to reach both screens",
    );

    const masterAlphaReady = () => {
        const current = actors(conn);
        if (!Array.isArray(current) || current.length !== 2)
            return false;
        for (const screen of current) {
            const rects = visibleRects(screen);
            if (rects.length !== 1)
                return false;
            const got = alpha(rects[0].style);
            if (got === null || Math.abs(got - 0.55) >= 0.01)
                return false;
        }
        return true;
    };

    waitUntil(masterAlphaReady, "both overlays to show master alpha 0.55");
    const masterTree = actors(conn);
    for (const screen of masterTree) {
        const rects = visibleRects(screen);
        check(
            rects.length === 1,
            `${screen.name} should show one dim rectangle`,
        );
        if (rects.length > 0) {
            const got = alpha(rects[0].style);
            check(
                got !== null && Math.abs(got - 0.55) < 0.01,
                `${screen.name} master alpha is ${got}, expected 0.55`,
            );
        }
    }

    // SetMaster(false, …) only releases the master lock: it keeps the dim
    // values master last applied instead of restoring the pre-master
    // per-screen values.
    callApp(conn, "SetMaster", new GLib.Variant("(bd)", [false, 30.0]), "()");
    waitUntil(() => !state(conn).master.on, "master control to turn off");
    const afterMaster = state(conn);
    check(
        Math.abs(afterMaster.master.dim - 30.0) < 0.01,
        `master dim is ${afterMaster.master.dim}, expected 30`,
    );
    check(
        afterMaster.screens.every(screen => Math.abs(screen.dim - 55.0) < 0.01),
        `master-off dims should stay at 55: ${afterMaster.screens.map(s => s.dim)}`,
    );

    const retainedAlphaReady = () => {
        const current = actors(conn);
        if (!Array.isArray(current) || current.length !== 2)
            return false;
        return current.every(screen => {
            const rects = visibleRects(screen);
            return (
                rects.length === 1 &&
                Math.abs((alpha(rects[0].style) ?? -1) - 0.55) < 0.01
            );
        });
    };

    waitUntil(retainedAlphaReady, "retained 0.55 overlays after master off");

    // Individual control must work again afterwards.
    callApp(conn, "SetScreen", new GLib.Variant("(ibd)", [1, true, 40.0]), "()");
    callApp(conn, "SetScreen", new GLib.Variant("(ibd)", [2, true, 60.0]), "()");
    waitUntil(
        () => {
            const dims = state(conn).screens.map(screen => screen.dim);
            return dims.length === 2 && dims[0] === 40.0 && dims[1] === 60.0;
        },
        "individual dims to return to 40/60",
    );

    const individualAlphaReady = () => {
        const current = actors(conn);
        if (!Array.isArray(current) || current.length !== 2)
            return false;
        for (const [screenId, want] of Object.entries(expectedAlpha)) {
            const rects = visibleRects(screenGroup(current, Number(screenId)));
            if (rects.length !== 1)
                return false;
            const got = alpha(rects[0].style);
            if (got === null || Math.abs(got - want) >= 0.01)
                return false;
        }
        return true;
    };

    waitUntil(
        individualAlphaReady,
        "individual overlays to return to 0.40/0.60",
    );
    print(
        "actor tree: two screens, labels, boundaries, per-screen and master alpha verified",
    );
}

main();
if (problems.length > 0) {
    for (const problem of problems)
        print(`FAIL: ${problem}`);
    System.exit(1);
}
print("PASS: two-monitor state, overlays, boundaries, IDs and master control");
