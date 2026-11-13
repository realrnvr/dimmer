// Pure maths for adaptive dimming.
//
// Everything here is free of GNOME Shell imports so it can be unit-tested
// under plain `gjs`, in the same style as `overlayMath.js`.
//
// Encoding note: Mutter composites in *encoded* sRGB, so a black overlay of
// alpha `a` satisfies `out = in * (1 - a)` per channel in that space.  All
// values in this module are encoded sRGB in [0, 1]; that keeps the model exact
// (a linear scale in the compositor's blend space) and makes the recovery in
// `recover()` exact rather than approximate.

/** Pixel budget for one monitor's sample grid (240x135 at 16:9). */
export const GRID_PIXELS = 32400;

/** Fraction of brightest pixels used by the high-percentile mean. */
export const PERCENTILE = 0.20;

/** Maximum additional darkness adaptive mode may add, before the total cap. */
export const MAX_ADAPTIVE_ALPHA = 0.50;

/** Existing overlay clamp: user dim + adaptive never exceeds this. */
export const MAX_TOTAL_ALPHA = 0.90;

/** Darkening speed, in alpha per second. */
export const ATTACK_RATE = 0.80;

/** Lightening speed, in alpha per second. Deliberately much slower. */
export const RELEASE_RATE = 0.10;

/** A target change smaller than this is ignored (prevents limit cycles). */
export const HYSTERESIS = 0.02;

/** Snap to the target once within this distance, so it settles exactly. */
export const SETTLE = 0.005;

const LUMA_RED = 0.2126;
const LUMA_GREEN = 0.7152;
const LUMA_BLUE = 0.0722;

/**
 * Clamp a value into a range, tolerating junk input.
 *
 * @param {number} value
 * @param {number} low
 * @param {number} high
 * @returns {number}
 */
export function clamp(value, low, high) {
    const number = Number(value);
    if (!Number.isFinite(number))
        return low;
    return Math.min(high, Math.max(low, number));
}

/**
 * Downscale factor that keeps a monitor's sample grid at a constant pixel
 * count, independent of resolution. Never upscales.
 *
 * @param {number} width logical width in stage units
 * @param {number} height logical height in stage units
 * @returns {number} a factor in [0.02, 1]
 */
export function gridScale(width, height) {
    const w = Number(width);
    const h = Number(height);
    if (!(w > 0) || !(h > 0))
        return 1;
    return clamp(Math.sqrt(GRID_PIXELS / (w * h)), 0.02, 1);
}

/**
 * Grid dimensions produced by `gridScale` for a rectangle.
 *
 * @param {number} width
 * @param {number} height
 * @returns {{width: number, height: number, scale: number}}
 */
export function gridSize(width, height) {
    const scale = gridScale(width, height);
    return {
        width: Math.max(1, Math.round(width * scale)),
        height: Math.max(1, Math.round(height * scale)),
        scale,
    };
}

/**
 * Quantise an alpha exactly the way St does (8-bit opacity), so that the value
 * used to *render* is the value used to *invert*.
 *
 * @param {number} alpha
 * @returns {number} a multiple of 1/255 in [0, 1]
 */
export function quantiseAlpha(alpha) {
    return Math.round(clamp(alpha, 0, 1) * 255) / 255;
}

/**
 * Sample interval for a given number of monitors, in milliseconds. Keeps the
 * total sampling cost per second roughly constant.
 *
 * @param {number} screenCount
 * @returns {number}
 */
export function sampleInterval(screenCount) {
    const screens = Math.max(1, Math.floor(Number(screenCount) || 1));
    return 250 * Math.max(1, Math.ceil(screens / 2));
}

/**
 * Build an inclusion mask for one monitor's grid.
 *
 * `exclusions` are stage-space rectangles whose pixels must not contribute:
 * the GNOME top panel (the overlay never covers it) and the non-dimmable hole
 * (which has neither the user's dim nor the adaptive layer).  Pixels outside
 * the mask are skipped, so they never need inversion either.
 *
 * @param {number} gridWidth
 * @param {number} gridHeight
 * @param {{x: number, y: number, width: number, height: number}} rect monitor rect
 * @param {Array<{x: number, y: number, width: number, height: number}>} exclusions
 * @returns {Uint8Array} 1 = include, 0 = skip
 */
export function buildMask(gridWidth, gridHeight, rect, exclusions) {
    const width = Math.max(1, Math.floor(gridWidth));
    const height = Math.max(1, Math.floor(gridHeight));
    const mask = new Uint8Array(width * height).fill(1);

    if (!rect || !Array.isArray(exclusions) || exclusions.length === 0)
        return mask;
    if (!(rect.width > 0) || !(rect.height > 0))
        return mask;

    const scaleX = width / rect.width;
    const scaleY = height / rect.height;

    for (const exclusion of exclusions) {
        if (!exclusion || !(exclusion.width > 0) || !(exclusion.height > 0))
            continue;
        const x0 = Math.max(0, Math.floor((exclusion.x - rect.x) * scaleX));
        const x1 = Math.min(width, Math.ceil((exclusion.x + exclusion.width - rect.x) * scaleX));
        const y0 = Math.max(0, Math.floor((exclusion.y - rect.y) * scaleY));
        const y1 = Math.min(height, Math.ceil((exclusion.y + exclusion.height - rect.y) * scaleY));
        for (let y = y0; y < y1; y++) {
            const row = y * width;
            for (let x = x0; x < x1; x++)
                mask[row + x] = 0;
        }
    }

    return mask;
}

/**
 * Statistics for one sample.
 *
 * `mean` is the arithmetic mean luma; `high` is the mean of the brightest
 * `PERCENTILE` fraction. Both are encoded sRGB in [0, 1] and both come from the
 * same 256-bin histogram, so they are exact to 1/255.
 *
 * @typedef {{mean: number, high: number, count: number, width: number, height: number}} Stats
 */

/**
 * Build a 256-bin luma histogram and derive the two statistics.
 *
 * @param {Uint8Array} pixels raw pixbuf bytes
 * @param {number} stride row stride in bytes
 * @param {number} channels bytes per pixel
 * @param {number} width
 * @param {number} height
 * @param {Uint8Array | null} mask optional inclusion mask, length width*height
 * @returns {Stats}
 */
export function histogramStats(pixels, stride, channels, width, height, mask = null) {
    const bins = new Float64Array(256);
    let count = 0;

    for (let y = 0; y < height; y++) {
        const row = y * stride;
        const maskRow = y * width;
        for (let x = 0; x < width; x++) {
            if (mask && mask[maskRow + x] === 0)
                continue;
            const offset = row + x * channels;
            const luma = LUMA_RED * pixels[offset] +
                LUMA_GREEN * pixels[offset + 1] +
                LUMA_BLUE * pixels[offset + 2];
            bins[Math.round(luma)]++;
            count++;
        }
    }

    if (count === 0)
        return {mean: 0, high: 0, count: 0, width, height};

    let sum = 0;
    for (let bin = 0; bin < 256; bin++)
        sum += bin * bins[bin];
    const mean = sum / count / 255;

    // Mean of the brightest PERCENTILE fraction, pro-rating the boundary bin.
    let need = count * PERCENTILE;
    let weighted = 0;
    let taken = 0;
    for (let bin = 255; bin >= 0 && need > 1e-9; bin--) {
        const take = Math.min(bins[bin], need);
        if (take <= 0)
            continue;
        weighted += take * bin;
        taken += take;
        need -= take;
    }

    return {
        mean: clamp(mean, 0, 1),
        high: taken > 0 ? clamp(weighted / taken / 255, 0, 1) : 0,
        count,
        width,
        height,
    };
}

/**
 * Undo the adaptive layer, recovering the pre-adaptive image statistics.
 *
 * Multiplying every pixel by the same positive constant preserves their
 * ordering, so the top-`PERCENTILE` subset is unchanged and the recovery is
 * exact for `high` as well as `mean` — no need to touch pixels.
 *
 * @param {Stats} stats statistics measured on the composited output
 * @param {number} appliedAlpha alpha actually rendered (already quantised)
 * @returns {Stats}
 */
export function recover(stats, appliedAlpha) {
    const alpha = quantiseAlpha(appliedAlpha);
    if (alpha <= 0)
        return stats;
    const divisor = 1 - alpha;
    if (divisor <= 1e-6)
        return {...stats, mean: 1, high: 1};
    return {
        ...stats,
        mean: clamp(stats.mean / divisor, 0, 1),
        high: clamp(stats.high / divisor, 0, 1),
    };
}

/**
 * Targets derived from the 0-100 strength setting.
 *
 * Strength moves the targets, never the cap, so a low strength still reaches
 * the cap on a fully white screen — it just needs more white area to get there.
 *
 * @param {number} strength 0-100
 * @returns {{high: number, mean: number, max: number}}
 */
export function targets(strength) {
    const s = clamp(strength, 0, 100) / 100;
    return {
        high: 0.72 - 0.20 * s,
        mean: 0.42 - 0.14 * s,
        max: MAX_ADAPTIVE_ALPHA,
    };
}

/**
 * How much extra darkness is still available under the 90 % total cap.
 *
 * @param {number} userDim the screen's effective dim percentage (0-90)
 * @returns {number} alpha in [0, MAX_ADAPTIVE_ALPHA]
 */
export function headroom(userDim) {
    const used = clamp(userDim, 0, MAX_TOTAL_ALPHA * 100) / 100;
    return Math.max(0, Math.min(MAX_ADAPTIVE_ALPHA, MAX_TOTAL_ALPHA - used));
}

/**
 * Smallest alpha that satisfies both limiters, clamped to the headroom.
 *
 * `max(rHigh, rMean)` is exact rather than a tuning choice: one scalar multiply
 * scales both statistics, so any alpha at or above both requirements satisfies
 * both, and the max is the smallest such alpha.
 *
 * @param {Stats} stats recovered (pre-adaptive) statistics
 * @param {{high: number, mean: number, max: number}} limit
 * @param {number} available headroom, from `headroom()`
 * @returns {number} alpha in [0, available]
 */
export function requiredAlpha(stats, limit, available) {
    const cap = Math.max(0, Math.min(available, limit.max));
    if (!(stats.count > 0) || cap <= 0)
        return 0;

    const high = stats.high;
    const mean = stats.mean;

    const rHigh = high > limit.high ? 1 - limit.high / high : 0;
    const rMean = mean > limit.mean ? 1 - limit.mean / mean : 0;

    return clamp(Math.max(rHigh, rMean), 0, cap);
}

/**
 * Convenience wrapper: measured statistics plus settings in, alpha out.
 *
 * @param {Stats} measured statistics on the composited output
 * @param {{on: boolean, strength: number}} settings
 * @param {number} userDim the screen's effective dim percentage
 * @param {number} appliedAlpha alpha currently rendered (0 if not painted)
 * @returns {{alpha: number, recovered: Stats, limit: object, cap: number}}
 */
export function adaptiveAlpha(measured, settings, userDim, appliedAlpha) {
    const limit = targets(settings?.strength ?? 50);
    const cap = headroom(userDim);
    if (!settings?.on || !(measured?.count > 0))
        return {alpha: 0, recovered: measured, limit, cap};

    const recovered = recover(measured, appliedAlpha);
    return {alpha: requiredAlpha(recovered, limit, cap), recovered, limit, cap};
}

/**
 * Move `current` toward `target` under the asymmetric rate limit.
 *
 * @param {number} current
 * @param {number} target
 * @param {number} dt seconds since the previous tick
 * @returns {number}
 */
export function rateLimit(current, target, dt) {
    const from = clamp(current, 0, 1);
    const to = clamp(target, 0, 1);
    const step = to - from;
    const limit = (step > 0 ? ATTACK_RATE : RELEASE_RATE) * clamp(dt, 0, 0.5);
    let next = from + clamp(step, -limit, limit);
    if (Math.abs(to - next) < SETTLE)
        next = to;
    return clamp(next, 0, 1);
}

/**
 * Whether a new target is far enough from the current one to be adopted.
 *
 * @param {number} current
 * @param {number} next
 * @returns {boolean}
 */
export function shouldUpdate(current, next) {
    if (next === 0 && current < HYSTERESIS)
        return true;
    return Math.abs(next - current) >= HYSTERESIS;
}

/**
 * Normalise the optional `adaptive` block from the state document.
 *
 * @param {unknown} adaptive
 * @returns {{on: boolean, strength: number}}
 */
export function parseAdaptive(adaptive) {
    if (!adaptive || typeof adaptive !== 'object')
        return {on: false, strength: 50};
    const raw = Number(adaptive.strength);
    return {
        on: adaptive.on === true,
        strength: Number.isFinite(raw) ? Math.round(clamp(raw, 0, 100)) : 50,
    };
}
