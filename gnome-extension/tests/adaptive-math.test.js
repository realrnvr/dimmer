// Unit tests for the adaptive-diming maths.
//
//   gjs -m gnome-extension/tests/adaptive-math.test.js
//
// Deliberately free of GNOME Shell imports: every case here runs on synthetic
// pixel buffers, so the calibration table can be checked without a compositor.

import {
    ATTACK_RATE,
    HYSTERESIS,
    MAX_ADAPTIVE_ALPHA,
    PERCENTILE,
    RELEASE_RATE,
    adaptiveAlpha,
    buildMask,
    clamp,
    gridScale,
    gridSize,
    headroom,
    histogramStats,
    parseAdaptive,
    quantiseAlpha,
    rateLimit,
    recover,
    requiredAlpha,
    sampleInterval,
    shouldUpdate,
    targets,
} from '../dimmer@dimmer.linux/adaptiveMath.js';

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

function near(actual, expected, tolerance, label) {
    if (!Number.isFinite(actual) || Math.abs(actual - expected) > tolerance) {
        fail(`${label}: expected ${expected} +/- ${tolerance}, got ${actual}`);
    }
}

function ok(value, label) {
    if (!value)
        fail(label);
}

const CHANNELS = 4;

/**
 * Synthetic RGBA pixbuf. The first `fraction * total` pixels are white and the
 * rest are black, which makes both statistics analytic:
 *   M = A,  H = min(1, A / PERCENTILE)
 */
function patch(fraction, width = 100, height = 100) {
    const total = width * height;
    const white = Math.round(clamp(fraction, 0, 1) * total);
    const pixels = new Uint8Array(total * CHANNELS);
    for (let index = 0; index < total; index++) {
        const offset = index * CHANNELS;
        const value = index < white ? 255 : 0;
        pixels[offset] = value;
        pixels[offset + 1] = value;
        pixels[offset + 2] = value;
        pixels[offset + 3] = 255;
    }
    return {pixels, width, height, stride: width * CHANNELS};
}

/** Composite a pure-black layer over a pixel buffer, like Mutter does. */
function composite(source, alpha) {
    const factor = 1 - quantiseAlpha(alpha);
    const pixels = new Uint8Array(source.pixels.length);
    for (let index = 0; index < source.pixels.length; index++)
        pixels[index] = Math.round(source.pixels[index] * factor);
    return {...source, pixels};
}

function statsOf(image, mask = null) {
    return histogramStats(
        image.pixels,
        image.stride,
        CHANNELS,
        image.width,
        image.height,
        mask);
}

// --- 1. uniform image -----------------------------------------------------

for (const value of [0, 1, 64, 128, 200, 255]) {
    const image = patch(0, 10, 10);
    for (let index = 0; index < image.pixels.length; index++)
        image.pixels[index] = index % CHANNELS === 3 ? 255 : value;
    const stats = statsOf(image);
    const expected = value / 255;
    near(stats.mean, expected, 1 / 255, `uniform ${value} mean`);
    near(stats.high, expected, 1 / 255, `uniform ${value} high`);
    equal(stats.count, 100, `uniform ${value} count`);
}

// --- 2. analytic white-patch model ----------------------------------------

for (const fraction of [0.01, 0.02, 0.05, 0.1, PERCENTILE, 0.15, 0.2, 0.35, 0.5, 0.6, 0.8, 1.0]) {
    const stats = statsOf(patch(fraction));
    near(stats.mean, fraction, 0.001, `patch ${fraction} mean`);
    near(stats.high, Math.min(1, fraction / PERCENTILE), 0.001,
        `patch ${fraction} high`);
}

// --- 3. calibration table (plan section 9) --------------------------------

const TABLE = [
    [0.01, 0.000],
    [0.05, 0.000],
    [0.10, 0.000],
    [0.124, 0.000],
    [0.15, 0.173],
    [0.20, 0.380],
    [0.35, 0.380],
    [0.50, 0.380],
    [0.60, 0.417],
    [0.80, 0.500],
    [1.00, 0.500],
];

for (const [fraction, expected] of TABLE) {
    const stats = statsOf(patch(fraction, 240, 135));
    const result = adaptiveAlpha(stats, {on: true, strength: 50}, 0, 0);
    near(result.alpha, expected, 0.002, `table A=${fraction}`);
}

// Threshold: nothing happens until the high-percentile mean passes 0.62.
{
    const stats = statsOf(patch(0.1235));
    equal(adaptiveAlpha(stats, {on: true, strength: 50}, 0, 0).alpha, 0,
        'threshold below knee');
    ok(statsOf(patch(0.13)).high > 0.62, 'threshold just above knee');
}

// --- 4. monotonicity ------------------------------------------------------

{
    let previous = -1;
    let increasing = true;
    for (let step = 0; step <= 1000; step++) {
        const fraction = step / 1000;
        const stats = statsOf(patch(fraction, 120, 120));
        const alpha = adaptiveAlpha(stats, {on: true, strength: 50}, 0, 0).alpha;
        if (alpha < previous - 1e-9)
            increasing = false;
        previous = alpha;
    }
    ok(increasing, 'alpha is non-decreasing in white area');
    near(previous, MAX_ADAPTIVE_ALPHA, 1e-9, 'alpha reaches the cap');
}

// --- 5. never brighten ----------------------------------------------------

{
    // Deterministic LCG so failures reproduce.
    let state = 0x2545f491;
    const random = () => {
        state = (state * 1664525 + 1013904223) >>> 0;
        return state / 0x100000000;
    };

    let monotoneViolations = 0;
    for (let trial = 0; trial < 10000; trial++) {
        const stats = {
            mean: random(),
            high: random(),
            count: 1 + Math.floor(random() * 30000),
            width: 240,
            height: 135,
        };
        // Keep the statistics physically ordered.
        if (stats.high < stats.mean)
            stats.high = stats.mean;

        const settings = {on: random() > 0.1, strength: Math.floor(random() * 101)};
        const userDim = Math.floor(random() * 91);
        const applied = random() * 0.9;
        const alpha = adaptiveAlpha(stats, settings, userDim, applied).alpha;

        if (!(alpha >= 0) || !(alpha <= headroom(userDim)) || alpha > MAX_ADAPTIVE_ALPHA)
            monotoneViolations++;
        if (!settings.on && alpha !== 0)
            monotoneViolations++;
    }
    equal(monotoneViolations, 0, 'never brightens / respects headroom');
}

// --- 6. headroom ----------------------------------------------------------

equal(headroom(0), 0.5, 'headroom at 0% dim');
equal(headroom(40), 0.5, 'headroom at 40% dim');
near(headroom(60), 0.3, 1e-9, 'headroom at 60% dim');
near(headroom(80), 0.1, 1e-9, 'headroom at 80% dim');
equal(headroom(90), 0, 'headroom at 90% dim');
equal(headroom(150), 0, 'headroom clamps high');
equal(headroom(-10), 0.5, 'headroom clamps low');
equal(headroom('nope'), 0.5, 'headroom rejects text');

{
    // A user already at 90 % gets nothing, and the result is reported as zero.
    const stats = statsOf(patch(1.0));
    const result = adaptiveAlpha(stats, {on: true, strength: 100}, 90, 0);
    equal(result.alpha, 0, 'no adaptation without headroom');
    equal(result.cap, 0, 'cap reported as zero');
}

// --- 7. inversion round-trip ---------------------------------------------

for (const alpha of [0, 0.02, 0.1, 0.25, 0.45, 0.6, 0.75, 0.9]) {
    const source = patch(0.37, 160, 90);
    const truth = statsOf(source);
    const observed = statsOf(composite(source, alpha));
    const recovered = recover(observed, alpha);
    near(recovered.mean, truth.mean, 0.005, `invert mean at ${alpha}`);
    near(recovered.high, truth.high, 0.005, `invert high at ${alpha}`);
}

{
    // Unquantised alpha must be normalised before inversion, or the result is
    // systematically wrong. 0.3 is not exactly representable in 8 bits.
    const source = patch(0.5, 80, 80);
    const truth = statsOf(source);
    const observed = statsOf(composite(source, 0.3));
    const careless = {...observed, mean: observed.mean / (1 - 0.3)};
    const careful = recover(observed, 0.3);
    ok(Math.abs(careful.mean - truth.mean) <= Math.abs(careless.mean - truth.mean),
        'quantised alpha is at least as accurate as the float');
    near(careful.mean, truth.mean, 0.003, 'quantised inversion');
}

// --- 8. strength mapping --------------------------------------------------

const STRENGTHS = [
    [0, 0.72, 0.42],
    [25, 0.67, 0.385],
    [50, 0.62, 0.35],
    [75, 0.57, 0.315],
    [100, 0.52, 0.28],
];

for (const [strength, high, mean] of STRENGTHS) {
    const limit = targets(strength);
    near(limit.high, high, 1e-9, `strength ${strength} high target`);
    near(limit.mean, mean, 1e-9, `strength ${strength} mean target`);
    equal(limit.max, MAX_ADAPTIVE_ALPHA, `strength ${strength} cap`);
}

near(targets(-50).high, 0.72, 1e-9, 'stress: strength clamps low');
near(targets(5000).high, 0.52, 1e-9, 'stress: strength clamps high');
near(targets('junk').high, 0.72, 1e-9, 'stress: strength rejects text');

// Higher strength must never dim less.
{
    const stats = statsOf(patch(0.4));
    let previous = -1;
    let increasing = true;
    for (let strength = 0; strength <= 100; strength++) {
        const alpha = adaptiveAlpha(stats, {on: true, strength}, 0, 0).alpha;
        if (alpha < previous - 1e-9)
            increasing = false;
        previous = alpha;
    }
    ok(increasing, 'alpha is non-decreasing in strength');
}

// --- 9. hysteresis --------------------------------------------------------

ok(shouldUpdate(0.2, 0.2 + HYSTERESIS + 0.001), 'hysteresis accepts large change');
ok(!shouldUpdate(0.2, 0.2 + HYSTERESIS - 0.001), 'hysteresis rejects small change');
ok(shouldUpdate(0.01, 0), 'hysteresis allows settling to off');
// A large drop is adopted immediately as the target; the rate limiter, not the
// hysteresis, is what makes the release itself gradual.
ok(shouldUpdate(0.5, 0), 'hysteresis adopts a large drop to off');
ok(shouldUpdate(0.0, 0.0), 'hysteresis accepts identical values');

// --- 10. rate limiter -----------------------------------------------------

{
    let alpha = 0;
    let steps = 0;
    while (alpha < 0.38 && steps < 1000) {
        alpha = rateLimit(alpha, 0.38, 0.01);
        steps++;
    }
    ok(steps >= 47 && steps <= 49, `attack reaches 0.38 in ${steps} x10ms steps`);
    near(alpha, 0.38, 1e-9, 'attack settles exactly');
}
{
    let alpha = 0.38;
    let steps = 0;
    while (alpha > 0 && steps < 10000) {
        alpha = rateLimit(alpha, 0, 0.01);
        steps++;
    }
    // 3.8 s, minus the SETTLE snap of the last 0.005 (five 10 ms steps).
    ok(steps >= 374 && steps <= 376, `release reaches 0 in ${steps} x10ms steps`);
    equal(alpha, 0, 'release settles exactly at zero');
}
{
    // Asymmetric: darkening is much faster than lightening.
    const up = rateLimit(0, 1, 0.1) - 0;
    const down = 1 - rateLimit(1, 0, 0.1);
    ok(up > down, 'attack outpaces release');
    near(up, ATTACK_RATE * 0.1, 1e-9, 'attack rate applied');
    near(down, RELEASE_RATE * 0.1, 1e-9, 'release rate applied');
}
{
    // dt is clamped, so a long stall cannot jump the whole range at once.
    near(rateLimit(0, 0.5, 30), ATTACK_RATE * 0.5, 1e-9, 'dt clamps at 0.5 s');
    equal(rateLimit(0.4, 0.4, 0.1), 0.4, 'no movement when already settled');
    equal(rateLimit(-1, 2, 0.1), rateLimit(0, 1, 0.1), 'inputs are clamped');
}

// --- 11. settings and grid helpers ---------------------------------------

equal(parseAdaptive(null), {on: false, strength: 50}, 'parseAdaptive null');
equal(parseAdaptive({}), {on: false, strength: 50}, 'parseAdaptive empty');
equal(parseAdaptive({on: true, strength: 70}), {on: true, strength: 70},
    'parseAdaptive valid');
equal(parseAdaptive({on: 'true', strength: '80'}), {on: false, strength: 80},
    'parseAdaptive only accepts a real boolean');
equal(parseAdaptive({on: true, strength: 500}), {on: true, strength: 100},
    'parseAdaptive clamps high');
equal(parseAdaptive({on: true, strength: -20}), {on: true, strength: 0},
    'parseAdaptive clamps low');
equal(parseAdaptive({on: true, strength: 62.6}), {on: true, strength: 63},
    'parseAdaptive rounds');
equal(parseAdaptive('nope'), {on: false, strength: 50}, 'parseAdaptive rejects text');

equal(gridSize(1920, 1080), {width: 240, height: 135, scale: 0.125},
    'gridSize 1080p');
equal(gridSize(1080, 1920), {width: 135, height: 240, scale: 0.125},
    'gridSize portrait');
equal(gridSize(1280, 720), {width: 240, height: 135, scale: 0.1875},
    'gridSize 720p');
equal(gridSize(3840, 2160).width, 240, 'gridSize 4K width');
equal(gridSize(100000, 100000).scale, 0.02, 'gridScale clamps low');
equal(gridScale(10, 10), 1, 'gridScale never upscales');
equal(gridScale(0, 0), 1, 'gridScale handles zero');
equal(gridScale(NaN, 100), 1, 'gridScale handles NaN');

equal(quantiseAlpha(0.3), Math.round(0.3 * 255) / 255, 'quantiseAlpha 0.3');
equal(quantiseAlpha(2), 1, 'quantiseAlpha clamps high');
equal(quantiseAlpha(-1), 0, 'quantiseAlpha clamps low');

equal(sampleInterval(1), 250, 'sampleInterval one monitor');
equal(sampleInterval(2), 250, 'sampleInterval two monitors');
equal(sampleInterval(3), 500, 'sampleInterval three monitors');
equal(sampleInterval(4), 500, 'sampleInterval four monitors');
equal(sampleInterval(0), 250, 'sampleInterval handles zero');

// --- mask -----------------------------------------------------------------

{
    const monitor = {x: 0, y: 0, width: 100, height: 100};
    const full = buildMask(10, 10, monitor, []);
    let included = 0;
    for (const value of full)
        included += value;
    equal(included, 100, 'empty mask includes everything');

    const masked = buildMask(10, 10, monitor, [{x: 0, y: 0, width: 100, height: 20}]);
    included = 0;
    for (const value of masked)
        included += value;
    equal(included, 80, 'top band excluded');

    // Exclusion outside the monitor must not wrap or corrupt rows.
    const outside = buildMask(10, 10, monitor, [{x: 500, y: 500, width: 10, height: 10}]);
    included = 0;
    for (const value of outside)
        included += value;
    equal(included, 100, 'outside exclusion is ignored');

    // A partial rect at non-zero origin.
    const offset = buildMask(10, 10, {x: 200, y: 100, width: 100, height: 100},
        [{x: 250, y: 150, width: 50, height: 50}]);
    included = 0;
    for (const value of offset)
        included += value;
    equal(included, 75, 'offset exclusion covers quarter');

    // Fully masked -> no pixels, so the controller holds instead of dividing.
    const everything = buildMask(4, 4, monitor, [monitor]);
    equal(statsOf(patch(1, 4, 4), everything).count, 0, 'fully masked count');
}

// --- statistics on masked input ------------------------------------------

{
    // Masking the white half must leave a pure black sample.
    const image = patch(0, 10, 10);
    for (let index = 0; index < 50; index++) {
        const offset = index * CHANNELS;
        image.pixels[offset] = 255;
        image.pixels[offset + 1] = 255;
        image.pixels[offset + 2] = 255;
    }
    const mask = buildMask(10, 10, {x: 0, y: 0, width: 10, height: 10},
        [{x: 0, y: 0, width: 10, height: 5}]);
    const stats = statsOf(image, mask);
    equal(stats.mean, 0, 'masked white band removed');
    equal(stats.count, 50, 'masked count');
}

// --- requiredAlpha edge cases --------------------------------------------

{
    const limit = targets(50);
    equal(requiredAlpha({mean: 0, high: 0, count: 0}, limit, 0.5), 0,
        'requiredAlpha handles empty samples');
    equal(requiredAlpha({mean: 1, high: 1, count: 10}, limit, 0), 0,
        'requiredAlpha handles zero headroom');
    // Uniform mid grey below both targets: nothing to do.
    equal(requiredAlpha({mean: 0.3, high: 0.3, count: 10}, limit, 0.5), 0,
        'requiredAlpha no-op below targets');
}

// --- result ---------------------------------------------------------------

if (failures > 0) {
    printerr(`adaptive-math: ${failures} failure(s)`);
    imports.system.exit(1);
}

print('adaptive-math: all checks passed');
