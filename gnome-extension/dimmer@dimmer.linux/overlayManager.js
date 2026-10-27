// Compositor-side rendering for the Dimmer overlays.
//
// One root actor lives at the top of `global.window_group`, i.e. above normal
// application windows but below all Shell chrome (panel, overview, lock
// screen, menus).  Each screen gets a group with up to four dim rectangles so
// the settings window can punch a hole when "Non-dimmable window" is enabled.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {layoutSignature, overlayColor, rectsForMonitor} from './overlayMath.js';
import {resetWindowFrame, windowMatchesResetFrame} from './windowReset.js';

const HOLE_REFRESH_MS = 150;
const WINDOW_RESET_RETRY_MS = 50;
const WINDOW_RESET_MAX_ATTEMPTS = 20;
const DEFAULT_WINDOW_WIDTH = 1040;
const DEFAULT_WINDOW_HEIGHT = 700;

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

function lower(value) {
    return typeof value === 'string' ? value.toLowerCase() : '';
}

export class OverlayManager {
    constructor() {
        this._enabled = false;
        this._root = null;
        this._screenActors = new Map();
        this._layout = '';
        this._state = null;
        this._overviewShowing = false;
        this._holeTimer = 0;
        this._lastHole = null;
        this._overviewShowingId = 0;
        this._overviewHidingId = 0;
        this._unredirectDisabled = null;
        this._windowResetTimer = 0;
        this._windowResetRequest = null;
    }

    enable() {
        if (this._enabled)
            return;
        this._enabled = true;

        this._root = new St.Widget({
            name: 'dimmer-overlay-root',
            reactive: false,
            can_focus: false,
            track_hover: false,
            visible: false,
        });
        this._updateRootSize();

        // window_group paints above normal windows.  Mutter lowers window
        // actors when it restacks, so this child stays on top, while the Shell
        // panel and other chrome (which live in uiGroup above window_group)
        // remain undimmed and interactive.
        global.window_group.add_child(this._root);
        global.window_group.set_child_above_sibling(this._root, null);

        this._overviewShowing = Main.overview.visible;
        this._overviewShowingId = Main.overview.connect('showing', () => {
            this._overviewShowing = true;
            this._syncVisibility();
        });
        this._overviewHidingId = Main.overview.connect('hiding', () => {
            this._overviewShowing = false;
            this._syncVisibility();
        });

        this._disableUnredirect();
    }

    disable() {
        if (!this._enabled)
            return;
        this._enabled = false;

        this._stopHoleTimer();
        this._stopWindowReset(true);
        this._destroyScreens();

        if (this._overviewShowingId !== 0) {
            Main.overview.disconnect(this._overviewShowingId);
            this._overviewShowingId = 0;
        }
        if (this._overviewHidingId !== 0) {
            Main.overview.disconnect(this._overviewHidingId);
            this._overviewHidingId = 0;
        }

        this._restoreUnredirect();

        if (this._root) {
            this._root.destroy();
            this._root = null;
        }

        this._state = null;
        this._layout = '';
        this._lastHole = null;
    }

    setState(state) {
        if (!this._enabled)
            return;

        this._state = state;
        this._updateHoleTimer();
        this._apply();
    }

    clear() {
        this._stopWindowReset(true);
        this._state = null;
        this._lastHole = null;
        this._stopHoleTimer();

        if (this._root)
            this._root.visible = false;
    }

    refresh() {
        if (this._enabled && this._state)
            this._apply();
    }

    resetWindowPosition(reportResult) {
        this._stopWindowReset(true);
        if (!this._enabled || !this._state) {
            reportResult(false);
            return;
        }

        this._windowResetRequest = {
            reportResult,
            frame: null,
            window: null,
            attempts: 0,
        };
        this._windowResetTimer = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            WINDOW_RESET_RETRY_MS,
            () => this._checkWindowReset());
    }

    _stopWindowReset(reportFailure = false) {
        if (this._windowResetTimer !== 0) {
            GLib.source_remove(this._windowResetTimer);
            this._windowResetTimer = 0;
        }
        const request = this._windowResetRequest;
        this._windowResetRequest = null;
        if (request && reportFailure)
            request.reportResult(false);
    }

    _checkWindowReset() {
        const request = this._windowResetRequest;
        if (!request || !this._enabled || !this._state) {
            this._stopWindowReset(true);
            return GLib.SOURCE_REMOVE;
        }

        if (!request.frame) {
            const window = this._findDimmerWindow(this._state);
            const monitorIndex = Main.layoutManager.primaryIndex;
            const workArea = Main.layoutManager.getWorkAreaForMonitor(monitorIndex);
            request.frame = resetWindowFrame(
                window,
                workArea,
                DEFAULT_WINDOW_WIDTH,
                DEFAULT_WINDOW_HEIGHT);
            if (request.frame) {
                request.window = window;
                request.attempts = 0;
            }
        } else if (windowMatchesResetFrame(request.window, request.frame)) {
            this._finishWindowReset(true);
            return GLib.SOURCE_REMOVE;
        }

        request.attempts++;
        if (request.attempts >= WINDOW_RESET_MAX_ATTEMPTS) {
            this._finishWindowReset(false);
            return GLib.SOURCE_REMOVE;
        }
        return GLib.SOURCE_CONTINUE;
    }

    _finishWindowReset(success) {
        const request = this._windowResetRequest;
        if (this._windowResetTimer !== 0) {
            GLib.source_remove(this._windowResetTimer);
            this._windowResetTimer = 0;
        }
        this._windowResetRequest = null;
        request?.reportResult(success);
    }

    // --- state -> actors --------------------------------------------------

    _apply() {
        const state = this._state;
        if (!state || !this._root) {
            this._syncVisibility();
            return;
        }

        this._updateRootSize();

        const signature = layoutSignature(state.screens);
        if (signature !== this._layout) {
            this._destroyScreens();
            this._buildScreens(state.screens);
            this._layout = signature;
        }

        const hole = this._holeActive() ? this._findDimmerWindowRect() : null;
        this._lastHole = hole;

        for (const screen of state.screens) {
            const entry = this._screenActors.get(screen.id);
            if (!entry)
                continue;

            const geometry = screen.geometry;
            if (!geometry)
                continue;

            const rects = state.non_dimmable_window && hole
                ? rectsForMonitor(geometry, hole)
                : [geometry];

            this._syncScreen(entry, screen, rects);
        }

        // Keep the overlay above window actors even after a restack.
        global.window_group.set_child_above_sibling(this._root, null);
        this._syncVisibility();
    }

    _buildScreens(screens) {
        const root = this._root;
        if (!root)
            return;

        const [stageWidth, stageHeight] = global.stage.get_size();

        for (const screen of screens) {
            const group = new St.Widget({
                name: `dimmer-overlay-screen-${screen.id}`,
                reactive: false,
                can_focus: false,
                track_hover: false,
            });
            group.set_size(stageWidth, stageHeight);
            root.add_child(group);

            const rectGroup = new St.Widget({
                name: `dimmer-overlay-rects-${screen.id}`,
                reactive: false,
                can_focus: false,
                track_hover: false,
            });
            rectGroup.set_size(stageWidth, stageHeight);
            group.add_child(rectGroup);

            const boundary = new St.Widget({
                name: `dimmer-overlay-boundary-${screen.id}`,
                style_class: 'dimmer-boundary',
                reactive: false,
                can_focus: false,
                track_hover: false,
                visible: false,
            });
            group.add_child(boundary);

            const label = new St.Label({
                name: `dimmer-overlay-id-${screen.id}`,
                style_class: 'dimmer-screen-id',
                text: String(screen.id),
                reactive: false,
                can_focus: false,
                track_hover: false,
                visible: false,
            });
            group.add_child(label);

            this._screenActors.set(screen.id, {
                screenId: screen.id,
                group,
                rectGroup,
                rects: [],
                boundary,
                label,
            });
        }
    }

    _syncScreen(entry, screen, rects) {
        const geometry = screen.geometry;
        const dim = Number(screen.dim) || 0;
        const dimmed = screen.on !== false && dim > 0;
        const colour = overlayColor(screen.tint ?? '000000', dim);

        while (entry.rects.length < rects.length) {
            const actor = new St.Widget({
                name: `dimmer-overlay-rect-${screen.id}-${entry.rects.length}`,
                reactive: false,
                can_focus: false,
                track_hover: false,
            });
            entry.rectGroup.add_child(actor);
            entry.rects.push(actor);
        }

        for (let index = 0; index < entry.rects.length; index++) {
            const actor = entry.rects[index];
            const rectangle = rects[index];
            if (!rectangle) {
                actor.visible = false;
                continue;
            }

            actor.set_position(rectangle.x, rectangle.y);
            actor.set_size(rectangle.width, rectangle.height);
            actor.set_style(`background-color: ${colour};`);
            actor.visible = dimmed;
        }

        entry.boundary.set_position(geometry.x, geometry.y);
        entry.boundary.set_size(geometry.width, geometry.height);
        entry.boundary.visible = Boolean(this._state?.show_boundaries);

        entry.label.set_position(geometry.x + 35, geometry.y + 30);
        entry.label.text = String(screen.id);
        entry.label.visible = Boolean(this._state?.show_ids);
    }

    _destroyScreens() {
        for (const entry of this._screenActors.values())
            entry.group.destroy();
        this._screenActors.clear();
    }

    _syncVisibility() {
        if (!this._root)
            return;

        const hasScreens = Boolean(this._state?.screens?.length);
        this._root.visible = this._enabled && hasScreens && !this._overviewShowing;
    }

    _updateRootSize() {
        if (!this._root)
            return;
        const [width, height] = global.stage.get_size();
        this._root.set_size(width, height);
        for (const entry of this._screenActors.values()) {
            entry.group.set_size(width, height);
            entry.rectGroup.set_size(width, height);
        }
    }

    // --- non-dimmable window hole ----------------------------------------

    _holeActive() {
        return Boolean(this._state?.non_dimmable_window && this._state?.window_visible);
    }

    _updateHoleTimer() {
        const active = this._enabled && this._holeActive();

        if (active && this._holeTimer === 0) {
            this._holeTimer = GLib.timeout_add(
                GLib.PRIORITY_DEFAULT,
                HOLE_REFRESH_MS,
                () => {
                    if (!this._enabled || !this._holeActive()) {
                        this._holeTimer = 0;
                        return GLib.SOURCE_REMOVE;
                    }

                    const hole = this._findDimmerWindowRect();
                    if (!sameRect(hole, this._lastHole)) {
                        this._lastHole = hole;
                        this._apply();
                    }
                    return GLib.SOURCE_CONTINUE;
                });
        } else if (!active) {
            this._stopHoleTimer();
        }
    }

    _stopHoleTimer() {
        if (this._holeTimer !== 0) {
            GLib.source_remove(this._holeTimer);
            this._holeTimer = 0;
        }
    }

    _findDimmerWindowRect() {
        const state = this._state;
        if (!state)
            return null;

        const window = this._findDimmerWindow(state, true);
        return window ? toRect(window.get_frame_rect()) : null;
    }

    _findDimmerWindow(state, visibleOnly = false) {
        const wantedAppId = lower(state.window_app_id);
        const wantedClass = lower(state.window_wm_class);
        if (!wantedAppId && !wantedClass)
            return null;

        const actors = global.get_window_actors();
        // get_window_actors() is ordered bottom-to-top; pick the topmost match.
        for (let index = actors.length - 1; index >= 0; index--) {
            const actor = actors[index];
            if (visibleOnly && !actor?.visible)
                continue;
            const window = actor.meta_window ?? actor.get_meta_window?.();
            if (!window || (visibleOnly && (window.minimized || window.is_hidden?.())))
                continue;
            if (visibleOnly && window.showing_on_its_workspace &&
                !window.showing_on_its_workspace())
                continue;
            if (!this._matchesWindow(window, wantedAppId, wantedClass))
                continue;
            return window;
        }

        return null;
    }

    _matchesWindow(window, wantedAppId, wantedClass) {
        const identifiers = [
            window.get_gtk_application_id?.(),
            window.get_sandboxed_app_id?.(),
            window.get_wm_class?.(),
            window.get_wm_class_instance?.(),
        ];
        for (const identifier of identifiers) {
            const value = lower(identifier);
            if (value && (value === wantedAppId || value === wantedClass))
                return true;
        }
        return false;
    }

    // --- compositor unredirect -------------------------------------------

    _disableUnredirect() {
        const compositor = global.compositor;
        if (typeof compositor?.disable_unredirect === 'function') {
            compositor.disable_unredirect();
            this._unredirectDisabled = () => {
                if (typeof compositor.enable_unredirect === 'function')
                    compositor.enable_unredirect();
            };
            return;
        }

        if (typeof Meta.disable_unredirect_for_display === 'function') {
            Meta.disable_unredirect_for_display(global.display);
            this._unredirectDisabled = () => {
                if (typeof Meta.enable_unredirect_for_display === 'function')
                    Meta.enable_unredirect_for_display(global.display);
            };
        }
    }

    _restoreUnredirect() {
        if (this._unredirectDisabled) {
            this._unredirectDisabled();
            this._unredirectDisabled = null;
        }
    }
}
