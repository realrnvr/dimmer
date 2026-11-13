// Adaptive dimming controller.
//
// Samples the screen four times a second, computes the additional darkness
// required to reach the configured targets, and feeds the overlay through an
// asymmetric rate limiter ten times a second.  Confined to the sensible middle:
// it never brightens, never exceeds the total-alpha cap, and pauses whenever
// the Shell owns the screen.

import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {
    adaptiveAlpha,
    clamp,
    parseAdaptive,
    rateLimit,
    sampleInterval,
    shouldUpdate,
} from './adaptiveMath.js';
import {capture, ensureCapability, unavailable} from './adaptiveCapture.js';

/** Controller tick, in milliseconds. */
const TICK_MS = 100;

/** Metrics are reported to the application at 2 Hz. */
const REPORT_INTERVAL_S = 0.5;

/** Consecutive sample failures before adaptive mode gives up for the session. */
const MAX_FAILURES = 3;

function toRect(rectangle) {
    if (!rectangle)
        return null;
    const {x, y, width, height} = rectangle;
    if (!Number.isFinite(x) || !Number.isFinite(y) ||
        !Number.isFinite(width) || !Number.isFinite(height) ||
        width <= 0 || height <= 0)
        return null;
    return {x, y, width, height};
}

function sameRect(a, b) {
    if (a === null || b === null)
        return a === b;
    return a.x === b.x && a.y === b.y &&
        a.width === b.width && a.height === b.height;
}

export class AdaptiveController {
    /**
     * @param {import('./overlayManager.js').OverlayManager} overlays
     * @param {{reportMetrics?: (payload: object) => void}} [options]
     */
    constructor(overlays, options = {}) {
        this._overlays = overlays;
        this._reportMetrics = options.reportMetrics ?? null;

        this._enabled = false;
        this._timer = 0;
        this._settings = {on: false, strength: 50};
        this._screens = new Map();
        this._paused = false;

        this._lastTick = 0;
        this._lastSample = 0;
        this._lastReport = 0;
        this._sampling = false;
        this._failures = 0;
        this._givenUp = false;
        this._available = null;
    }

    enable() {
        if (this._enabled)
            return;
        this._enabled = true;
        this._givenUp = false;
        this._failures = 0;
        this._lastTick = GLib.get_monotonic_time() / 1e6;
        this._lastSample = this._lastTick;
        this._lastReport = this._lastTick;

        ensureCapability().then(api => {
            this._available = Boolean(api);
            if (!api && this._enabled)
                console.debug(`[dimmer] adaptive dimming off: ${unavailable()}`);
        });

        this._timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, TICK_MS, () => this._tick());
    }

    disable() {
        if (!this._enabled)
            return;
        this._enabled = false;

        if (this._timer !== 0) {
            GLib.source_remove(this._timer);
            this._timer = 0;
        }

        this._clear();
        this._screens.clear();
        this._paused = false;
    }

    /**
     * Adopt a new state document.  Geometry changes reset that screen's alpha
     * so a moved monitor cannot inherit stale darkness.
     *
     * @param {object} state
     */
    setState(state) {
        const previous = this._screens;
        const next = new Map();
        this._settings = parseAdaptive(state?.adaptive);

        for (const screen of Array.isArray(state?.screens) ? state.screens : []) {
            const geometry = toRect(screen?.geometry);
            const userDim = clamp(Number(screen?.dim) || 0, 0, 90);
            const eligible = Boolean(geometry) && screen?.on !== false && userDim > 0;
            const old = previous.get(screen.id);
            const stable = Boolean(old) && sameRect(old.geometry, geometry) &&
                old.userDim === userDim;

            // A moved or re-dimmed monitor must not inherit stale darkness,
            // including the value cached inside the overlay manager.
            if (!stable || !eligible)
                this._overlays.setAdaptiveAlpha(screen.id, 0);

            next.set(screen.id, {
                id: screen.id,
                geometry,
                userDim,
                eligible,
                alpha: stable ? old.alpha : 0,
                target: stable ? old.target : 0,
                metrics: old?.metrics ?? null,
            });
        }

        // Screens that disappeared must not leave an invisible layer behind.
        for (const id of previous.keys()) {
            if (!next.has(id))
                this._overlays.setAdaptiveAlpha(id, 0);
        }

        this._screens = next;

        if (!this._settings.on) {
            this._clear();
            this._screens.clear();
            this._overlays.setAdaptiveAlpha(null, 0);
        }
    }

    /** Live metrics for the Debug tab, keyed by screen id. */
    metrics() {
        const result = {};
        for (const entry of this._screens.values()) {
            result[entry.id] = {
                high: entry.metrics?.high ?? null,
                mean: entry.metrics?.mean ?? null,
                alpha: entry.alpha,
                sampleMs: entry.metrics?.sampleMs ?? null,
            };
        }
        return result;
    }

    // --- loop -------------------------------------------------------------

    _tick() {
        if (!this._enabled)
            return GLib.SOURCE_REMOVE;

        const now = GLib.get_monotonic_time() / 1e6;
        const dt = this._lastTick === 0 ? TICK_MS / 1000 : now - this._lastTick;
        this._lastTick = now;

        this._paused = this._shouldPause();

        for (const entry of this._screens.values()) {
            if (!this._settings.on || !entry.eligible) {
                if (entry.alpha !== 0 || entry.target !== 0) {
                    entry.alpha = 0;
                    entry.target = 0;
                    this._overlays.setAdaptiveAlpha(entry.id, 0);
                }
                continue;
            }

            if (this._paused)
                continue;

            const next = rateLimit(entry.alpha, entry.target, dt);
            if (next !== entry.alpha) {
                entry.alpha = next;
                this._overlays.setAdaptiveAlpha(entry.id, next);
            }
        }

        if (!this._paused && !this._givenUp && this._settings.on &&
            !this._sampling && now - this._lastSample >= this._interval() / 1000) {
            this._sampleAll();
        }

        if (now - this._lastReport >= REPORT_INTERVAL_S) {
            this._lastReport = now;
            this._report();
        }

        return GLib.SOURCE_CONTINUE;
    }

    _interval() {
        return sampleInterval(this._screens.size);
    }

    /**
     * The Shell owns the screen in these states; freezing (rather than
     * releasing) avoids a visible jump behind a menu or dialog.
     */
    _shouldPause() {
        if (Main.sessionMode?.isLocked)
            return true;
        if (Main.overview?.visible)
            return true;
        if ((Main.modalCount ?? 0) > 0)
            return true;
        if (Main.panel?.menuManager?.activeMenu)
            return true;
        return false;
    }

    async _sampleAll() {
        this._sampling = true;
        try {
            const entries = [...this._screens.values()].filter(entry =>
                this._settings.on && entry.eligible);

            for (const entry of entries) {
                if (!this._enabled || this._givenUp)
                    return;
                // State may have changed while the previous sample was in flight.
                if (!this._screens.has(entry.id))
                    continue;

                try {
                    const result = await capture(entry.geometry, this._exclusions(entry));
                    if (!this._enabled)
                        return;
                    this._failures = 0;
                    this._available = true;
                    this._applySample(entry, result);
                } catch (error) {
                    this._handleFailure(error);
                    if (this._givenUp)
                        return;
                }
            }

            this._lastSample = GLib.get_monotonic_time() / 1e6;
        } finally {
            this._sampling = false;
        }
    }

    _applySample(entry, result) {
        // Only invert the layer if it actually rendered.  When the overlay is
        // not mapped (e.g. a headless session) the capture already shows the
        // undimmed content, and dividing by (1 - alpha) would over-brighten it.
        const applied = this._overlays.isPainted?.() ? entry.alpha : 0;
        const {alpha, recovered, cap} = adaptiveAlpha(
            result.stats, this._settings, entry.userDim, applied);

        if (shouldUpdate(entry.target, alpha))
            entry.target = alpha;

        entry.metrics = {
            high: recovered?.high ?? null,
            mean: recovered?.mean ?? null,
            sampleMs: result.sampleMs,
            cap,
        };
    }

    _handleFailure(error) {
        this._failures++;
        if (this._failures === 1)
            console.debug(`[dimmer] adaptive sample failed: ${error?.message ?? error}`);

        if (this._failures >= MAX_FAILURES) {
            this._givenUp = true;
            this._available = false;
            console.warn('[dimmer] adaptive dimming disabled after repeated capture failures');
            this._clear();
        }
    }

    _exclusions() {
        const rects = [];

        const panel = Main.panel;
        if (panel?.visible) {
            const [x, y] = panel.get_transformed_position();
            const [width, height] = panel.get_transformed_size();
            if (width > 0 && height > 0)
                rects.push({x, y, width, height});
        }

        const hole = this._overlays.holeRect?.();
        if (hole)
            rects.push(hole);

        return rects;
    }

    _report() {
        if (!this._reportMetrics)
            return;

        const screens = [];
        for (const entry of this._screens.values()) {
            if (!entry.eligible)
                continue;
            screens.push({
                id: entry.id,
                high: entry.metrics?.high ?? null,
                mean: entry.metrics?.mean ?? null,
                alpha: entry.alpha,
                sample_ms: entry.metrics?.sampleMs ?? null,
            });
        }

        try {
            this._reportMetrics({
                available: this._available !== false,
                paused: this._paused,
                screens,
            });
        } catch (_error) {
            // Telemetry must never disturb dimming.
        }
    }

    _clear() {
        for (const entry of this._screens.values()) {
            if (entry.alpha !== 0 || entry.target !== 0) {
                entry.alpha = 0;
                entry.target = 0;
            }
        }
        this._overlays.clearAdaptive?.();
    }
}
