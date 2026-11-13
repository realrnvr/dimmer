// Screen-content sampling for adaptive dimming.
//
// One synchronous stage capture at a fixed pixel budget per monitor, decoded
// through GdkPixbuf and reduced to two statistics.  No screenshot is ever
// written to disk, kept between samples, or exposed outside this module.
//
// Two things here are deliberate and load-bearing:
//
//   * `paint_to_content` and `composite_to_stream` have changed across the
//     GNOME 45-51 range, and `Function.length` cannot tell the variants apart
//     (it reports 4 for a 3-argument method).  So the first working combination
//     is discovered by trying them, once, and cached.
//   * The texture returned by `paint_to_content().get_texture()` is owned by
//     the content.  Calling `run_dispose()` on it crashes the Shell, so it is
//     dropped and left to GC.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import {buildMask, gridSize, histogramStats} from './adaptiveMath.js';

const MAX_DETECT_ATTEMPTS = 3;

let capability = null;
let detectPromise = null;
let detectAttempts = 0;
let unavailableReason = 'not probed yet';

function paintFlag() {
    return Clutter.PaintFlag?.NO_CURSORS ?? 0;
}

/**
 * Rectangle constructors differ between releases: `Mtk.Rectangle` on current
 * shells, `Meta.Rectangle` on older ones.  Anything else (plain objects,
 * `Clutter.Rect`) is rejected by the GI marshaller, so only real structs are
 * collected here.
 *
 * @returns {Promise<Array<{label: string, make: function(object): object}>>}
 */
async function loadRectFactories() {
    const factories = [];

    for (const uri of ['gi://Mtk', 'gi://Meta']) {
        try {
            // Dynamic so a missing namespace is a skipped candidate, not a
            // module-load failure that would take the extension down.
            const module = await import(uri);
            const namespace = module?.default ?? module;
            const Rectangle = namespace?.Rectangle;
            if (typeof Rectangle === 'function')
                factories.push({label: uri, make: rect => new Rectangle(rect)});
        } catch (_error) {
            // Namespace not present on this shell version.
        }
    }

    return factories;
}

/**
 * Argument orders seen in the wild.  Newer Clutter takes an explicit colour
 * state; older builds take only the flags.
 */
const FORMS = [
    {
        label: 'rect,scale,color,flags',
        paint: (stage, rect, scale) =>
            stage.paint_to_content(rect, scale, null, paintFlag()),
    },
    {
        label: 'rect,scale,flags',
        paint: (stage, rect, scale) =>
            stage.paint_to_content(rect, scale, paintFlag()),
    },
];

async function detect() {
    if (typeof global?.stage?.paint_to_content !== 'function') {
        unavailableReason = 'Clutter.Stage.paint_to_content is unavailable';
        return null;
    }
    if (typeof Shell.Screenshot?.composite_to_stream !== 'function') {
        unavailableReason = 'Shell.Screenshot.composite_to_stream is unavailable';
        return null;
    }

    const factories = await loadRectFactories();
    if (factories.length === 0) {
        unavailableReason = 'no supported rectangle constructor';
        return null;
    }

    for (const factory of factories) {
        for (const form of FORMS) {
            try {
                const probe = factory.make({x: 0, y: 0, width: 32, height: 32});
                const content = form.paint(global.stage, probe, 1);
                const texture = content?.get_texture?.();
                if (texture && texture.get_width() > 0) {
                    console.debug(`[dimmer] adaptive capture via ${factory.label} (${form.label})`);
                    return {
                        makeRect: factory.make,
                        paint: form.paint,
                        label: `${factory.label} (${form.label})`,
                    };
                }
            } catch (_error) {
                // Combination unsupported; try the next one.
            }
        }
    }

    unavailableReason = 'no working paint_to_content signature';
    return null;
}

/**
 * Discover and cache the working capture API.  Success is cached for the
 * session; failure is retried up to `MAX_DETECT_ATTEMPTS` times so a transient
 * problem during `enable()` cannot disable the feature permanently.
 *
 * @returns {Promise<object | null>}
 */
export function ensureCapability() {
    if (capability)
        return Promise.resolve(capability);
    if (detectAttempts >= MAX_DETECT_ATTEMPTS)
        return Promise.resolve(null);
    if (detectPromise)
        return detectPromise;

    detectAttempts++;
    detectPromise = detect()
        .then(result => {
            detectPromise = null;
            if (result)
                capability = result;
            else if (detectAttempts >= MAX_DETECT_ATTEMPTS)
                console.warn(`[dimmer] adaptive dimming unavailable: ${unavailableReason}`);
            return result;
        })
        .catch(error => {
            detectPromise = null;
            unavailableReason = error?.message ?? String(error);
            return null;
        });

    return detectPromise;
}

/**
 * Why capture is not available, for the Debug tab and log messages.
 *
 * @returns {string}
 */
export function unavailable() {
    return unavailableReason;
}

/**
 * Capture one monitor and reduce it to statistics.
 *
 * @param {{x: number, y: number, width: number, height: number}} rect
 * @param {Array<object>} exclusions stage rects to ignore (panel, hole)
 * @returns {Promise<{stats: object, sampleMs: number, width: number, height: number}>}
 */
export async function capture(rect, exclusions = []) {
    const api = await ensureCapability();
    if (!api)
        throw new Error(unavailableReason);

    const started = GLib.get_monotonic_time();
    const {scale} = gridSize(rect.width, rect.height);

    const content = api.paint(global.stage, api.makeRect(rect), scale);
    if (!content)
        throw new Error('paint_to_content returned no content');

    const texture = content.get_texture();
    if (!texture)
        throw new Error('captured content had no texture');

    const textureWidth = texture.get_width();
    const textureHeight = texture.get_height();
    if (!(textureWidth > 0) || !(textureHeight > 0))
        throw new Error('captured an empty texture');

    const stream = Gio.MemoryOutputStream.new_resizable();
    const pixbuf = await new Promise((resolve, reject) => {
        try {
            Shell.Screenshot.composite_to_stream(
                texture,
                0, 0, textureWidth, textureHeight, 1,
                null, 0, 0, 1,
                stream,
                (_source, result) => {
                    try {
                        resolve(Shell.Screenshot.composite_to_stream_finish(result));
                    } catch (error) {
                        reject(error);
                    }
                });
        } catch (error) {
            reject(error);
        }
    });

    if (!pixbuf)
        throw new Error('composite_to_stream returned no pixbuf');

    const width = pixbuf.get_width();
    const height = pixbuf.get_height();
    const mask = buildMask(width, height, rect, exclusions);
    const stats = histogramStats(
        pixbuf.get_pixels(),
        pixbuf.get_rowstride(),
        pixbuf.get_n_channels(),
        width,
        height,
        mask);

    stream.close(null);

    return {
        stats,
        sampleMs: (GLib.get_monotonic_time() - started) / 1000,
        width,
        height,
    };
}
