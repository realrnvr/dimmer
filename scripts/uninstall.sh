#!/usr/bin/env bash
# Remove a per-user Dimmer installation.  Settings are kept unless --purge is used.
set -euo pipefail

PREFIX=${PREFIX:-$HOME/.local}
BINDIR=${BINDIR:-$PREFIX/bin}
DATADIR=${DATADIR:-$PREFIX/share}
CONFIGDIR=${XDG_CONFIG_HOME:-$HOME/.config}
PURGE=0

usage() {
    cat <<EOF
Usage: scripts/uninstall.sh [OPTIONS]

Removes the Dimmer binary, desktop files and GNOME Shell extension installed by
scripts/install.sh.

Options:
  --prefix <dir>  Installation prefix (default: \$HOME/.local)
  --purge         Also remove Dimmer settings
  -h, --help      Show this help

Environment:
  PREFIX, BINDIR, DATADIR can override the default paths.
  XDG_CONFIG_HOME selects where settings are stored.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || { echo "uninstall.sh: --prefix requires a directory" >&2; exit 2; }
            PREFIX=$2
            BINDIR=$PREFIX/bin
            DATADIR=$PREFIX/share
            shift 2
            ;;
        --prefix=*)
            PREFIX=${1#*=}
            BINDIR=$PREFIX/bin
            DATADIR=$PREFIX/share
            shift
            ;;
        --purge)
            PURGE=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "uninstall.sh: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

EXTENSION_DIR=$DATADIR/gnome-shell/extensions/dimmer@dimmer.linux
AUTOSTART=$CONFIGDIR/autostart/io.github.dimmer.Linux.desktop
SETTINGS=$CONFIGDIR/dimmer

if [ -x "$BINDIR/dimmer" ]; then
    "$BINDIR/dimmer" --quit >/dev/null 2>&1 || true
fi
if command -v gnome-extensions >/dev/null 2>&1; then
    XDG_DATA_HOME=$DATADIR gnome-extensions disable dimmer@dimmer.linux >/dev/null 2>&1 || true
fi

remove_file() {
    if [ -e "$1" ]; then
        rm -f "$1"
        echo "  removed $1"
    fi
}

remove_tree() {
    if [ -d "$1" ]; then
        rm -rf "$1"
        echo "  removed $1"
    fi
}

echo "Removing Dimmer from $PREFIX"
remove_file "$BINDIR/dimmer"
remove_file "$DATADIR/applications/io.github.dimmer.Linux.desktop"
remove_file "$DATADIR/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg"
remove_file "$DATADIR/metainfo/io.github.dimmer.Linux.metainfo.xml"
remove_tree "$EXTENSION_DIR"
remove_file "$AUTOSTART"

if [ "$PURGE" = 1 ]; then
    remove_tree "$SETTINGS"
else
    echo
    echo "Settings were kept in $SETTINGS (use --purge to remove them)."
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DATADIR/applications" >/dev/null 2>&1 || true
fi

echo "Uninstall complete."
