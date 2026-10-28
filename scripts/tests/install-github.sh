#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT

FIXTURES=$TEMP/fixtures
MOCK_BIN=$TEMP/bin
mkdir -p "$FIXTURES/bundle/data/icons/hicolor/scalable/apps" \
    "$FIXTURES/bundle/scripts" "$MOCK_BIN"
printf '#!/bin/sh\nexit 0\n' > "$FIXTURES/bundle/dimmer"
chmod +x "$FIXTURES/bundle/dimmer"
cp "$ROOT/scripts/install.sh" "$FIXTURES/bundle/scripts/install.sh"
cp "$ROOT/data/io.github.dimmer.Linux.desktop" "$FIXTURES/bundle/data/"
cp "$ROOT/data/io.github.dimmer.Linux.metainfo.xml" "$FIXTURES/bundle/data/"
cp "$ROOT/data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" \
    "$FIXTURES/bundle/data/icons/hicolor/scalable/apps/"
tar -C "$FIXTURES/bundle" -czf "$FIXTURES/dimmer-linux-x86_64.tar.gz" \
    dimmer scripts data
(cd "$FIXTURES" && sha256sum dimmer-linux-x86_64.tar.gz > SHA256SUMS)

cat > "$MOCK_BIN/curl" <<'MOCK_CURL'
#!/usr/bin/env bash
set -euo pipefail
url=
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) output=$2; shift 2 ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
case "$url" in
    */dimmer-linux-x86_64.tar.gz) cp "$INSTALL_TEST_FIXTURES/dimmer-linux-x86_64.tar.gz" "$output" ;;
    */SHA256SUMS) cp "$INSTALL_TEST_FIXTURES/SHA256SUMS" "$output" ;;
    *) echo "Unexpected test URL: $url" >&2; exit 1 ;;
esac
MOCK_CURL
chmod +x "$MOCK_BIN/curl"

export INSTALL_TEST_FIXTURES=$FIXTURES
export PATH=$MOCK_BIN:$PATH

bash "$ROOT/scripts/install-github.sh" --prefix "$TEMP/prefix" --no-enable
[ -x "$TEMP/prefix/bin/dimmer" ]
[ -f "$TEMP/prefix/share/applications/io.github.dimmer.Linux.desktop" ]
[ -f "$TEMP/prefix/share/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" ]
[ -f "$TEMP/prefix/share/metainfo/io.github.dimmer.Linux.metainfo.xml" ]

printf '%064d  dimmer-linux-x86_64.tar.gz\n' 0 > "$FIXTURES/SHA256SUMS"
if bash "$ROOT/scripts/install-github.sh" --prefix "$TEMP/rejected" >"$TEMP/output" 2>&1; then
    echo 'Installer accepted an archive with an invalid checksum.' >&2
    exit 1
fi
[ ! -e "$TEMP/rejected/bin/dimmer" ] || {
    echo 'Installer wrote files after a checksum failure.' >&2
    exit 1
}

echo 'GitHub installer checks passed.'
