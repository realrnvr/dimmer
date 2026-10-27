#!/usr/bin/env bash
# Build an installable GNOME Shell extension bundle.
#
# `gnome-extensions pack` only bundles metadata.json, extension.js and
# stylesheet.css by default; ES modules must be listed with --extra-source or
# the resulting zip is silently incomplete.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SOURCE=$ROOT/gnome-extension/dimmer@dimmer.linux
OUT=${1:-$ROOT/dist}
EXPECTED=(
    dbusClient.js
    extension.js
    indicator.js
    metadata.json
    overlayManager.js
    overlayMath.js
    stylesheet.css
    windowReset.js
)

if ! command -v gnome-extensions >/dev/null 2>&1; then
    echo "pack-extension.sh: gnome-extensions is required" >&2
    exit 1
fi
if ! command -v unzip >/dev/null 2>&1; then
    echo "pack-extension.sh: unzip is required for the content check" >&2
    exit 1
fi

mkdir -p "$OUT"
gnome-extensions pack --force --out-dir "$OUT" \
    --extra-source=dbusClient.js \
    --extra-source=indicator.js \
    --extra-source=overlayManager.js \
    --extra-source=overlayMath.js \
    --extra-source=windowReset.js \
    "$SOURCE"

BUNDLE=$OUT/dimmer@dimmer.linux.shell-extension.zip
actual=$(unzip -Z1 "$BUNDLE" | grep -v '/$' | sort)
for name in "${EXPECTED[@]}"; do
    if ! grep -qx "$name" <<<"$actual"; then
        echo "pack-extension.sh: $name is missing from $BUNDLE" >&2
        exit 1
    fi
done
count=$(wc -l <<<"$actual")
if [ "$count" -ne "${#EXPECTED[@]}" ]; then
    echo "pack-extension.sh: unexpected file count $count in $BUNDLE" >&2
    echo "$actual" >&2
    exit 1
fi

echo "Packed $BUNDLE"
echo "$actual" | sed 's/^/  /'
