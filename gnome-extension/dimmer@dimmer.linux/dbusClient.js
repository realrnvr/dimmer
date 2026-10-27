// D-Bus client for the Dimmer Rust service.
//
// The extension is deliberately "dumb": it watches the well-known name, asks
// the application for a state snapshot, then forwards changes to the overlay
// manager and the panel indicator.  All settings logic lives in the Rust app.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';

export const BUS_NAME = 'io.github.dimmer.Linux';
export const OBJECT_PATH = '/io/github/dimmer/Linux';
export const INTERFACE_NAME = 'io.github.dimmer.Linux';
export const PROTOCOL_VERSION = 1;

const CALL_TIMEOUT_MS = 2000;
const REGISTER_ATTEMPTS = 20;
const REGISTER_RETRY_MS = 100;

const debug = message => console.debug(`[dimmer] ${message}`);

export const DimmerDbusClient = GObject.registerClass({
    GTypeName: 'DimmerLinuxDbusClient',
    Signals: {
        'state-changed': {param_types: [GObject.TYPE_JSOBJECT]},
        'visibility-changed': {param_types: [GObject.TYPE_BOOLEAN]},
        'connected': {},
        'disconnected': {},
    },
}, class DimmerDbusClient extends GObject.Object {
    constructor(extensionVersion) {
        super();

        this._extensionVersion = String(extensionVersion);
        this._bus = Gio.DBus.session;
        this._cancellable = new Gio.Cancellable();
        this._watchId = 0;
        this._signalIds = [];
        this._retryId = 0;
        this._connected = false;
        this._destroyed = false;
        this._warnedInvalidState = false;
        this._state = null;
    }

    get connected() {
        return this._connected;
    }

    get state() {
        return this._state;
    }

    enable() {
        if (this._destroyed || this._watchId !== 0)
            return;

        this._watchId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            BUS_NAME,
            Gio.BusNameWatcherFlags.NONE,
            () => this._onAppeared(),
            () => this._onVanished());
    }

    destroy() {
        if (this._destroyed)
            return;

        this._destroyed = true;
        this._cancelRetry();
        this._unsubscribeSignals();

        if (this._watchId !== 0) {
            Gio.bus_unwatch_name(this._watchId);
            this._watchId = 0;
        }

        this._cancellable.cancel();
        this._connected = false;
        this._state = null;
    }

    // --- public commands --------------------------------------------------

    showWindow() {
        this._call('ShowWindow');
    }

    resetWindowPosition() {
        this._call('ResetWindowPosition');
    }

    windowResetCompleted(serial, success) {
        this._call('WindowResetCompleted',
            new GLib.Variant('(sb)', [String(serial), Boolean(success)]));
    }

    quit() {
        this._call('Quit');
    }

    extensionDisconnected() {
        if (this._destroyed || !this._bus)
            return;

        // Use a fire-and-forget call without the client cancellable so the
        // notification still reaches the app while the extension shuts down.
        this._bus.call(
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            'ExtensionDisconnected',
            null,
            null,
            Gio.DBusCallFlags.NONE,
            CALL_TIMEOUT_MS,
            null,
            () => {});
    }

    // Master/screen setters are kept for scripts and integration tests.  The
    // top-bar panel no longer exposes master controls; they live in the app
    // window.
    setMaster(on, dim) {
        if (!Number.isFinite(dim))
            return;
        this._call('SetMaster', new GLib.Variant('(bd)', [Boolean(on), dim]));
    }

    setScreen(id, on, dim) {
        if (!Number.isInteger(id) || !Number.isFinite(dim))
            return;
        this._call('SetScreen', new GLib.Variant('(ibd)', [id, Boolean(on), dim]));
    }

    // --- name watch -------------------------------------------------------

    _onAppeared() {
        if (this._destroyed)
            return;

        debug(`${BUS_NAME} appeared`);
        this._cancelRetry();
        this._subscribeSignals();
        this._register(0);
    }

    _onVanished() {
        if (this._destroyed)
            return;

        debug(`${BUS_NAME} vanished`);
        this._cancelRetry();
        this._unsubscribeSignals();

        const wasConnected = this._connected;
        this._connected = false;
        this._state = null;
        if (wasConnected)
            this.emit('disconnected');
    }

    // --- registration/state ----------------------------------------------

    _register(attempt) {
        if (this._destroyed)
            return;

        this._call('RegisterExtension',
            new GLib.Variant('(s)', [this._extensionVersion]),
            reply => {
                if (this._destroyed)
                    return;

                const json = reply?.deep_unpack?.()[0];
                if (typeof json === 'string' && this._acceptState(json)) {
                    this._markConnected();
                    return;
                }

                if (attempt + 1 < REGISTER_ATTEMPTS) {
                    this._retryId = GLib.timeout_add(
                        GLib.PRIORITY_DEFAULT,
                        REGISTER_RETRY_MS,
                        () => {
                            this._retryId = 0;
                            this._register(attempt + 1);
                            return GLib.SOURCE_REMOVE;
                        });
                    return;
                }

                debug('RegisterExtension failed, trying GetState');
                this._getStateFallback();
            });
    }

    _getStateFallback() {
        this._call('GetState', null, reply => {
            if (this._destroyed)
                return;

            const json = reply?.deep_unpack?.()[0];
            if (typeof json === 'string' && this._acceptState(json)) {
                this._markConnected();
                return;
            }

            debug('GetState failed; overlays remain hidden');
        });
    }

    _acceptState(json) {
        let state;
        try {
            state = JSON.parse(json);
        } catch (error) {
            if (!this._warnedInvalidState) {
                this._warnedInvalidState = true;
                console.warn(`[dimmer] invalid state JSON: ${error.message}`);
            }
            return false;
        }

        if (!state || typeof state !== 'object' ||
            state.protocol !== PROTOCOL_VERSION ||
            !Array.isArray(state.screens) ||
            !state.master || typeof state.master !== 'object') {
            if (!this._warnedInvalidState) {
                this._warnedInvalidState = true;
                console.warn('[dimmer] unexpected state document; ignoring');
            }
            return false;
        }

        this._state = state;
        return true;
    }

    _markConnected() {
        const wasConnected = this._connected;
        this._connected = true;
        if (!wasConnected)
            this.emit('connected');
        if (this._state)
            this.emit('state-changed', this._state);
    }

    // --- signals ----------------------------------------------------------

    _subscribeSignals() {
        if (this._signalIds.length > 0 || !this._bus)
            return;

        this._signalIds.push(this._bus.signal_subscribe(
            BUS_NAME,
            INTERFACE_NAME,
            'StateChanged',
            OBJECT_PATH,
            null,
            Gio.DBusSignalFlags.NONE,
            (_connection, _sender, _path, _iface, _signal, parameters) => {
                const json = parameters?.deep_unpack?.()[0];
                if (typeof json !== 'string' || !this._acceptState(json))
                    return;
                this.emit('state-changed', this._state);
            }));

        this._signalIds.push(this._bus.signal_subscribe(
            BUS_NAME,
            INTERFACE_NAME,
            'VisibilityChanged',
            OBJECT_PATH,
            null,
            Gio.DBusSignalFlags.NONE,
            (_connection, _sender, _path, _iface, _signal, parameters) => {
                const [visible] = parameters?.deep_unpack?.() ?? [false];
                this.emit('visibility-changed', Boolean(visible));
            }));
    }

    _unsubscribeSignals() {
        if (!this._bus) {
            this._signalIds = [];
            return;
        }

        for (const id of this._signalIds)
            this._bus.signal_unsubscribe(id);
        this._signalIds = [];
    }

    // --- plumbing ---------------------------------------------------------

    _cancelRetry() {
        if (this._retryId !== 0) {
            GLib.source_remove(this._retryId);
            this._retryId = 0;
        }
    }

    _call(method, parameters = null, callback = null) {
        if (this._destroyed || !this._bus)
            return;

        this._bus.call(
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            method,
            parameters,
            null,
            Gio.DBusCallFlags.NONE,
            CALL_TIMEOUT_MS,
            this._cancellable,
            (connection, result) => {
                if (this._destroyed)
                    return;

                try {
                    const reply = connection.call_finish(result);
                    callback?.(reply);
                } catch (error) {
                    if (error.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                        return;
                    debug(`${method} failed: ${error.message}`);
                    callback?.(null);
                }
            });
    }
});
