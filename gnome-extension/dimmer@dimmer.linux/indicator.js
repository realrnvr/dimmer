// Top-bar indicator.  Mirrors the original Windows tray menu (Open, Reset
// window position, Close).  Master dimming is a window control, not a panel
// control, so the panel stays a launcher/status menu.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

export const DimmerIndicator = GObject.registerClass({
    GTypeName: 'DimmerLinuxIndicator',
}, class DimmerIndicator extends PanelMenu.Button {
    constructor(client) {
        super(0.5, 'Dimmer', false);

        this._client = client;
        this._connected = false;
        this._disposed = false;

        this.connect('destroy', () => {
            this._disposed = true;
        });

        this.add_style_class_name('dimmer-indicator');
        this._icon = new St.Icon({
            icon_name: 'display-brightness-symbolic',
            style_class: 'system-status-icon',
        });
        this.add_child(this._icon);

        this._buildMenu();

        this._client.connectObject(
            'connected', () => this.updateConnection(true),
            'disconnected', () => this.updateConnection(false),
            this);
    }

    destroy() {
        this._disposed = true;
        this._client = null;
        super.destroy();
    }

    _buildMenu() {
        this._statusItem = new PopupMenu.PopupMenuItem('Dimmer is not running', {
            reactive: false,
        });
        this.menu.addMenuItem(this._statusItem);

        this._openItem = new PopupMenu.PopupMenuItem('Open Dimmer');
        this._openItem.connect('activate', () => this._open());
        this.menu.addMenuItem(this._openItem);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._resetItem = new PopupMenu.PopupMenuItem('Reset window position');
        this._resetItem.connect('activate', () => this._client?.resetWindowPosition());
        this.menu.addMenuItem(this._resetItem);

        this._quitItem = new PopupMenu.PopupMenuItem('Quit Dimmer');
        this._quitItem.connect('activate', () => this._client?.quit());
        this.menu.addMenuItem(this._quitItem);

        this.updateConnection(false);
    }

    updateConnection(connected) {
        if (this._disposed)
            return;

        this._connected = connected;
        this._statusItem.visible = !connected;
        this._openItem.label.text = connected ? 'Open Dimmer' : 'Start Dimmer';
        this._resetItem.sensitive = connected;
        this._quitItem.sensitive = connected;
    }

    async _open() {
        if (this._connected) {
            this._client?.showWindow();
            return;
        }

        // GLib 2.80 moved desktop entries to GioUnix.  Prefer it when present
        // so the Shell log stays clean, but keep the old Gio path for GNOME 45
        // and other older GLib versions.  Do not touch Gio.DesktopAppInfo
        // unless the import fails: reading it logs a deprecation warning.
        let DesktopAppInfo = null;
        try {
            const {default: GioUnix} = await import('gi://GioUnix');
            DesktopAppInfo = GioUnix.DesktopAppInfo ?? null;
        } catch {
            DesktopAppInfo = Gio.DesktopAppInfo ?? null;
        }

        if (DesktopAppInfo) {
            try {
                const appInfo = DesktopAppInfo.new('io.github.dimmer.Linux.desktop');
                if (appInfo) {
                    appInfo.launch([], null);
                    return;
                }
            } catch (error) {
                console.warn(`[dimmer] desktop launch failed: ${error.message}`);
            }
        }

        // The Shell's own PATH often omits ~/.local/bin, which is exactly
        // where the per-user installer puts the binary. Try the usual
        // locations before giving up.
        const candidates = [
            GLib.find_program_in_path('dimmer'),
            GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'dimmer']),
            '/usr/local/bin/dimmer',
            '/usr/bin/dimmer',
        ];
        const binary = candidates.find(path =>
            path && GLib.file_test(path, GLib.FileTest.IS_EXECUTABLE));
        if (binary) {
            try {
                Gio.Subprocess.new([binary, '--show'], Gio.SubprocessFlags.NONE);
                return;
            } catch (error) {
                console.warn(`[dimmer] could not start ${binary}: ${error.message}`);
            }
        }

        Main.notify('Dimmer', 'Could not start Dimmer. Is it installed?');
    }
});
