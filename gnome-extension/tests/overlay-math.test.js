// Pure helper tests.  Run with: gjs -m gnome-extension/tests/overlay-math.test.js
//
// These deliberately avoid GNOME Shell imports so the geometry logic can be
// checked without a compositor.

import {
    intersectRect,
    layoutSignature,
    overlayColor,
    parseTint,
    rectsForMonitor,
} from '../dimmer@dimmer.linux/overlayMath.js';

let failures = 0;

function fail(message) {
    failures += 1;
    printerr(`FAIL: ${message}`);
}

function equal(actual, expected, label) {
    const left = JSON.stringify(actual);
    const right = JSON.stringify(expected);
    if (left !== right)
        fail(`${label}: expected ${right}, got ${left}`);
}

function isNull(actual, label) {
    if (actual !== null)
        fail(`${label}: expected null, got ${JSON.stringify(actual)}`);
}

function area(rect) {
    return rect.width * rect.height;
}

function assertRectsValid(rects, monitor, label) {
    let covered = 0;
    for (const rect of rects) {
        if (rect.width <= 0 || rect.height <= 0)
            fail(`${label}: non-positive rectangle ${JSON.stringify(rect)}`);
        if (rect.x < monitor.x || rect.y < monitor.y ||
            rect.x + rect.width > monitor.x + monitor.width ||
            rect.y + rect.height > monitor.y + monitor.height)
            fail(`${label}: rectangle escapes monitor ${JSON.stringify(rect)}`);
        covered += area(rect);
    }
    for (let i = 0; i < rects.length; i++) {
        for (let j = i + 1; j < rects.length; j++) {
            if (intersectRect(rects[i], rects[j]))
                fail(`${label}: rectangles ${i} and ${j} overlap`);
        }
    }
    return covered;
}

// --- tint parsing ---------------------------------------------------------

equal(parseTint('000000'), [0, 0, 0], 'parseTint black');
equal(parseTint('#1a2B3c'), [26, 43, 60], 'parseTint with hash and mixed case');
equal(parseTint('ffffff'), [255, 255, 255], 'parseTint white');
equal(parseTint('xyz'), [0, 0, 0], 'parseTint invalid text');
equal(parseTint('12345'), [0, 0, 0], 'parseTint short value');
equal(parseTint(null), [0, 0, 0], 'parseTint null');
equal(parseTint(undefined), [0, 0, 0], 'parseTint undefined');

// --- overlay colours ------------------------------------------------------

equal(overlayColor('000000', 30), 'rgba(0, 0, 0, 0.300)', 'overlayColor 30%');
equal(overlayColor('ff8000', 0), 'rgba(255, 128, 0, 0.000)', 'overlayColor 0%');
equal(overlayColor('ff8000', 90), 'rgba(255, 128, 0, 0.900)', 'overlayColor 90%');
equal(overlayColor('ff8000', 250), 'rgba(255, 128, 0, 0.900)', 'overlayColor clamps high');
equal(overlayColor('ff8000', -5), 'rgba(255, 128, 0, 0.000)', 'overlayColor clamps low');
equal(overlayColor('ff8000', 'nope'), 'rgba(255, 128, 0, 0.000)', 'overlayColor rejects text');

// --- rectangle intersection ----------------------------------------------

equal(intersectRect(
    {x: 0, y: 0, width: 100, height: 100},
    {x: 50, y: 25, width: 100, height: 100}),
{x: 50, y: 25, width: 50, height: 75}, 'intersectRect overlap');

isNull(intersectRect(
    {x: 0, y: 0, width: 100, height: 100},
    {x: 100, y: 0, width: 50, height: 50}), 'intersectRect edge touch');

isNull(intersectRect(
    {x: 0, y: 0, width: 100, height: 100},
    {x: 200, y: 200, width: 50, height: 50}), 'intersectRect disjoint');

equal(intersectRect(
    {x: 0, y: 0, width: 100, height: 100},
    {x: 10, y: 10, width: 20, height: 20}),
{x: 10, y: 10, width: 20, height: 20}, 'intersectRect contained');

// --- hole splitting -------------------------------------------------------

const monitor = {x: 0, y: 0, width: 1920, height: 1080};

equal(rectsForMonitor(monitor, null), [monitor], 'rectsForMonitor without hole');

equal(rectsForMonitor(monitor, {x: 2000, y: 0, width: 100, height: 100}),
    [monitor], 'rectsForMonitor outside hole');

const centreHole = {x: 400, y: 300, width: 800, height: 500};
const centreRects = rectsForMonitor(monitor, centreHole);
equal(centreRects.length, 4, 'rectsForMonitor centre hole count');
let covered = assertRectsValid(centreRects, monitor, 'centre hole');
equal(covered, area(monitor) - area(centreHole), 'rectsForMonitor centre hole coverage');

const allHole = {x: -10, y: -10, width: 2000, height: 1200};
equal(rectsForMonitor(monitor, allHole), [], 'rectsForMonitor full cover');

const topLeftHole = {x: -50, y: -50, width: 300, height: 200};
const topLeftRects = rectsForMonitor(monitor, topLeftHole);
assertRectsValid(topLeftRects, monitor, 'top-left hole');
equal(topLeftRects.length, 2, 'rectsForMonitor top-left hole count');
equal(area(topLeftRects[0]) + area(topLeftRects[1]),
    area(monitor) - 250 * 150, 'rectsForMonitor top-left hole coverage');

const bottomRightHole = {x: 1700, y: 900, width: 500, height: 500};
const bottomRightRects = rectsForMonitor(monitor, bottomRightHole);
assertRectsValid(bottomRightRects, monitor, 'bottom-right hole');
equal(bottomRightRects.length, 2, 'rectsForMonitor bottom-right hole count');

const narrowHole = {x: 900, y: 100, width: 100, height: 800};
const narrowRects = rectsForMonitor(monitor, narrowHole);
equal(narrowRects.length, 4, 'rectsForMonitor tall hole count');
assertRectsValid(narrowRects, monitor, 'tall hole');
equal(narrowRects.reduce((sum, rect) => sum + area(rect), 0),
    area(monitor) - area(narrowHole), 'rectsForMonitor tall hole coverage');

// Negative monitor coordinates must work the same way.
const leftMonitor = {x: -1920, y: 0, width: 1920, height: 1080};
const leftHole = {x: -1700, y: 200, width: 600, height: 400};
const leftRects = rectsForMonitor(leftMonitor, leftHole);
equal(leftRects.length, 4, 'rectsForMonitor negative monitor count');
assertRectsValid(leftRects, leftMonitor, 'negative monitor');
equal(leftRects.reduce((sum, rect) => sum + area(rect), 0),
    area(leftMonitor) - area(leftHole), 'rectsForMonitor negative monitor coverage');

// --- layout signature -----------------------------------------------------

const screens = [
    {
        id: 1,
        connector: 'eDP-1',
        geometry: {x: 0, y: 0, width: 1920, height: 1080},
        scale: 1.0,
    },
    {
        id: 2,
        connector: 'DP-1',
        geometry: {x: 1920, y: 0, width: 2560, height: 1440},
        scale: 1.0,
    },
];

const signature = layoutSignature(screens);
equal(signature, '1:eDP-1:0:0:1920:1080:1|2:DP-1:1920:0:2560:1440:1',
    'layoutSignature content');
equal(layoutSignature(screens), signature, 'layoutSignature stable');

const moved = screens.map(screen => ({...screen}));
moved[1].geometry = {...moved[1].geometry, x: 1920, width: 2560};
equal(layoutSignature(moved), signature, 'layoutSignature unchanged geometry');

moved[1].geometry = {...moved[1].geometry, height: 1081};
if (layoutSignature(moved) === signature)
    fail('layoutSignature should change with height');
equal(layoutSignature([]), '', 'layoutSignature empty');
equal(layoutSignature(null), '', 'layoutSignature null');

// --- result ---------------------------------------------------------------

if (failures > 0) {
    printerr(`overlay-math: ${failures} failure(s)`);
    imports.system.exit(1);
}

print('overlay-math: all checks passed');
