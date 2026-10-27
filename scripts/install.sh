#!/usr/bin/env bash
# Install Dimmer for the current user.  No root privileges are required.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

PREFIX=${PREFIX:-$HOME/.local}
BINDIR=${BINDIR:-$PREFIX/bin}
DATADIR=${DATADIR:-$PREFIX/share}
BINARY=
DO_BUILD=1
DO_EXTENSION=1
DO_ENABLE=1

usage() {
    cat <<EOF
Usage: scripts/install.sh [OPTIONS]

Installs Dimmer into \$PREFIX (default: ~/.local).

Options:
  --prefix <dir>     Installation prefix (default: \$HOME/.local)
  --binary <file>    Install this binary instead of building a release build
  --no-build         Do not build; use target/release/dimmer
  --no-extension     Do not install the GNOME Shell extension
  --no-enable        Do not try to enable the extension with gnome-extensions
  -h, --help         Show this help

Environment:
  PREFIX, BINDIR, DATADIR can override the default paths.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            [ $# -ge 2 ] || { echo "install.sh: --prefix requires a directory" >&2; exit 2; }
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
        --binary)
            [ $# -ge 2 ] || { echo "install.sh: --binary requires a file" >&2; exit 2; }
            BINARY=$2
            shift 2
            ;;
        --binary=*)
            BINARY=${1#*=}
            shift
            ;;
        --no-build)
            DO_BUILD=0
            shift
            ;;
        --no-extension)
            DO_EXTENSION=0
            shift
            ;;
        --no-enable)
            DO_ENABLE=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "install.sh: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ -z "$BINARY" ]; then
    if [ "$DO_BUILD" = 1 ]; then
        echo "Building Dimmer (release)…"
        cargo build --release --manifest-path "$ROOT/Cargo.toml"
        BINARY=$ROOT/target/release/dimmer
    else
        BINARY=$ROOT/target/release/dimmer
    fi
fi
[ -x "$BINARY" ] || { echo "install.sh: binary not found or not executable: $BINARY" >&2; exit 1; }

echo "Installing Dimmer into $PREFIX"
install -Dm755 "$BINARY" "$BINDIR/dimmer"
# GNOME Shell does not always have ~/.local/bin on its PATH, so `Exec=dimmer`
# can fail even though the app is installed. Write the absolute path into the
# per-user desktop file; the template keeps the relative form for system
# packaging.
desktop_tmp=$(mktemp)
trap 'rm -f "$desktop_tmp"' EXIT
sed -e "s|^Exec=.*|Exec=\"$BINDIR/dimmer\"|" \
    -e "s|^TryExec=.*|TryExec=$BINDIR/dimmer|" \
    "$ROOT/data/io.github.dimmer.Linux.desktop" > "$desktop_tmp"
install -Dm644 "$desktop_tmp" \
    "$DATADIR/applications/io.github.dimmer.Linux.desktop"
install -Dm644 "$ROOT/data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" \
    "$DATADIR/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg"
install -Dm644 "$ROOT/data/io.github.dimmer.Linux.metainfo.xml" \
    "$DATADIR/metainfo/io.github.dimmer.Linux.metainfo.xml"

if [ "$DO_EXTENSION" = 1 ]; then
    XDG_DATA_HOME=$DATADIR "$BINDIR/dimmer" --install-extension
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DATADIR/applications" >/dev/null 2>&1 || true
fi

if [ "$DO_ENABLE" = 1 ] && command -v gnome-extensions >/dev/null 2>&1; then
    # Newly copied extensions are not known to the running Shell until the next
    # login, so failure here is expected and not fatal.
    XDG_DATA_HOME=$DATADIR gnome-extensions enable dimmer@dimmer.linux >/dev/null 2>&1 || true
fi

cat <<EOF

Installed:
  $BINDIR/dimmer
  $DATADIR/applications/io.github.dimmer.Linux.desktop
  $DATADIR/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg
  $DATADIR/metainfo/io.github.dimmer.Linux.metainfo.xml
EOF
if [ "$DO_EXTENSION" = 1 ]; then
    echo "  $DATADIR/gnome-shell/extensions/dimmer@dimmer.linux"
fi

case ":$PATH:" in
    *":$BINDIR:"*) ;;
    *)
        echo
        echo "Add $BINDIR to PATH, for example:"
        echo "  echo 'export PATH=\"$BINDIR:\$PATH\"' >> ~/.bashrc"
        ;;
esac

if [ "$DO_EXTENSION" = 1 ]; then
    cat <<EOF

GNOME Shell loads a newly installed extension only at the next login.
Log out and back in, then enable it if necessary:
  gnome-extensions enable dimmer@dimmer.linux
EOF
fi

cat <<EOF

Start Dimmer with:
  dimmer
EOF
