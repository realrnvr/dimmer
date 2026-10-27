#!/usr/bin/env bash
# Run Dimmer's end-to-end GUI tests in isolated Wayland sessions.
#
# Every suite runs in its own private D-Bus bus and XDG runtime directory.
# That isolation matters for `donate`: WayDriver's mock portal must own
# `org.freedesktop.portal.Desktop` before Dimmer starts, or the real
# xdg-desktop-portal would be D-Bus-activated first and swallow the request.
#
# Requirements (Fedora):
#   pkg-config glib2-devel gstreamer1-devel gstreamer1-plugins-base-devel
#   mutter pipewire wireplumber dbus-daemon at-spi2-core
#   gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugins-pipewire
#
# Usage:
#   gui-tests/run.sh              # all suites
#   gui-tests/run.sh header       # one suite
#   gui-tests/run.sh header pages # several suites
#
# Environment:
#   DIMMER_BIN        override the application binary (default: target/release/dimmer)
#   DIMMER_GST_DEVEL  optional sysroot containing extracted gstreamer *-devel RPMs
set -euo pipefail

DIR=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$DIR/.." && pwd)

if [ -n "${DIMMER_GST_DEVEL:-}" ]; then
    export PKG_CONFIG_PATH="$DIMMER_GST_DEVEL/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

if ! pkg-config --exists gstreamer-1.0 gstreamer-app-1.0; then
    echo "error: GStreamer development files not found." >&2
    echo "Install: sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel" >&2
    exit 1
fi

if [ -z "${DIMMER_BIN:-}" ]; then
    echo "building dimmer (release)…" >&2
    cargo build --release --manifest-path "$ROOT/Cargo.toml"
    export DIMMER_BIN="$ROOT/target/release/dimmer"
fi

if [ ! -x "$DIMMER_BIN" ]; then
    echo "error: DIMMER_BIN is not executable: $DIMMER_BIN" >&2
    exit 1
fi

if [ "$#" -eq 0 ]; then
    set -- donate header pages screens autostart
fi

# xdg-document-portal can still hold a FUSE mount for a moment after the bus
# exits. Retry, but never let temporary-directory cleanup fail the test run.
cleanup_runtime() {
    local dir=$1
    for _ in 1 2 3 4 5; do
        rm -rf "$dir" 2>/dev/null && return 0
        sleep 0.2
    done
    echo "warning: could not remove $dir" >&2
}

status=0
for suite in "$@"; do
    runtime=$(mktemp -d "${TMPDIR:-/tmp}/dimmer-gui-$suite.XXXXXX")
    chmod 700 "$runtime"

    echo "==> $suite (XDG_RUNTIME_DIR=$runtime)" >&2
    if ! XDG_RUNTIME_DIR="$runtime" dbus-run-session -- bash "$DIR/run-session.sh" "$suite"; then
        status=$?
        cleanup_runtime "$runtime"
        exit "$status"
    fi
    cleanup_runtime "$runtime"
done
