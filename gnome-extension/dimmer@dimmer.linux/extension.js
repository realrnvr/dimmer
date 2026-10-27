// GNOME Shell adapter for Dimmer for Linux.
//
// The extension renders the overlay state published by the Rust application
// and applies GNOME-specific window placement requests. It owns no settings.

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {DimmerDbusClient} from './dbusClient.js';
import {DimmerIndicator} from './indicator.js';
import {OverlayManager} from './overlayManager.js';

export default class DimmerExtension extends Extension {
    enable() {
        this._client = new DimmerDbusClient(this.metadata.version ?? 1);
        this._overlays = new OverlayManager();
        this._indicator = new DimmerIndicator(this._client);
        this._lastWindowResetSerial = null;

        this._overlays.enable();
        Main.panel.addToStatusArea(this.uuid, this._indicator, 0, 'right');

        this._clientIds = [
            this._client.connect('state-changed', (_client, state) => {
                this._overlays?.setState(state);
                const resetSerial = Number(state.window_reset_serial ?? 0);
                if (this._lastWindowResetSerial !== null &&
                    resetSerial !== this._lastWindowResetSerial)
                    this._overlays?.resetWindowPosition(
                        success => this._client?.windowResetCompleted(resetSerial, success));
                this._lastWindowResetSerial = resetSerial;
            }),
            this._client.connect('visibility-changed', () => {
                // StateChanged usually follows immediately; refreshing here
                // closes the hole as soon as the window is hidden.
                this._overlays?.refresh();
            }),
            this._client.connect('disconnected', () => {
                this._overlays?.clear();
            }),
        ];

        this._client.enable();
    }

    disable() {
        if (this._client && this._clientIds) {
            for (const id of this._clientIds)
                this._client.disconnect(id);
        }
        this._clientIds = null;
        this._lastWindowResetSerial = null;

        this._client?.extensionDisconnected();
        this._client?.destroy();
        this._client = null;

        this._overlays?.disable();
        this._overlays = null;

        this._indicator?.destroy();
        this._indicator = null;
    }
}
