#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT

FIXTURES=$TEMP/fixtures
MOCK_BIN=$TEMP/bin
WGET_BIN=$TEMP/wget-bin
mkdir -p "$FIXTURES/bundle/data/icons/hicolor/scalable/apps" \
    "$FIXTURES/bundle/scripts" "$MOCK_BIN" "$WGET_BIN"
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
if [ "${INSTALL_TEST_DELAY_ARCHIVE:-}" = yes ] \
    && [[ "$url" == */dimmer-linux-x86_64.tar.gz ]]; then
    sleep 0.9
fi
if [ "${INSTALL_TEST_FAIL_DOWNLOAD:-}" = archive ] \
    && [[ "$url" == */dimmer-linux-x86_64.tar.gz ]]; then
    exit 22
fi
case "$url" in
    */dimmer-linux-x86_64.tar.gz) cp "$INSTALL_TEST_FIXTURES/dimmer-linux-x86_64.tar.gz" "$output" ;;
    */SHA256SUMS) cp "$INSTALL_TEST_FIXTURES/SHA256SUMS" "$output" ;;
    *) echo "Unexpected test URL: $url" >&2; exit 1 ;;
esac
MOCK_CURL
chmod +x "$MOCK_BIN/curl"

cat > "$WGET_BIN/wget" <<'MOCK_WGET'
#!/usr/bin/env bash
set -euo pipefail
url=
output=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output-document=*) output=${1#*=}; shift ;;
        --quiet) shift ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
if [ "${INSTALL_TEST_DELAY_ARCHIVE:-}" = yes ] \
    && [[ "$url" == */dimmer-linux-x86_64.tar.gz ]]; then
    sleep 0.9
fi
if [ "${INSTALL_TEST_FAIL_DOWNLOAD:-}" = archive ] \
    && [[ "$url" == */dimmer-linux-x86_64.tar.gz ]]; then
    exit 22
fi
case "$url" in
    */dimmer-linux-x86_64.tar.gz) cp "$INSTALL_TEST_FIXTURES/dimmer-linux-x86_64.tar.gz" "$output" ;;
    */SHA256SUMS) cp "$INSTALL_TEST_FIXTURES/SHA256SUMS" "$output" ;;
    *) echo "Unexpected test URL: $url" >&2; exit 1 ;;
esac
MOCK_WGET
chmod +x "$WGET_BIN/wget"

for tool in bash cp dirname env grep install mkdir mktemp pwd rm sed sha256sum sleep tar; do
    ln -s "$(command -v "$tool")" "$WGET_BIN/$tool"
done

export INSTALL_TEST_FIXTURES=$FIXTURES
export PATH=$MOCK_BIN:$PATH

bash "$ROOT/scripts/install-github.sh" --prefix "$TEMP/prefix" --no-enable \
    > "$TEMP/non-tty.log" 2>&1
grep -F '[INFO]:' "$TEMP/non-tty.log" >/dev/null
! grep -F '[ERROR]:' "$TEMP/non-tty.log" >/dev/null
python3 - "$TEMP/non-tty.log" <<'PY'
import pathlib
import sys

output = pathlib.Path(sys.argv[1]).read_bytes()
assert b"\r" not in output, "non-terminal install emitted spinner control characters"
PY
[ -x "$TEMP/prefix/bin/dimmer" ]
[ -f "$TEMP/prefix/share/applications/io.github.dimmer.Linux.desktop" ]
[ -f "$TEMP/prefix/share/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" ]
[ -f "$TEMP/prefix/share/metainfo/io.github.dimmer.Linux.metainfo.xml" ]

if command -v script >/dev/null 2>&1; then
    INSTALL_TEST_DELAY_ARCHIVE=yes
    export INSTALL_TEST_DELAY_ARCHIVE
    script -qec "bash '$ROOT/scripts/install-github.sh' --prefix '$TEMP/tty-prefix' --no-enable" \
        /dev/null > "$TEMP/tty.log" 2>&1
    python3 - "$TEMP/tty.log" <<'PY'
import pathlib
import re
import sys

output = pathlib.Path(sys.argv[1]).read_bytes()
frames = re.findall(rb"\[INFO\]: Downloading release archive (\.+)", output)
assert frames[:4] == [b".", b"..", b"...", b"."], frames
assert b"[INFO]: Download complete." in output
PY

    bash_bin=$(command -v bash)
    script_bin=$(command -v script)
    "$script_bin" -qec "PATH='$WGET_BIN' '$bash_bin' '$ROOT/scripts/install-github.sh' --prefix '$TEMP/wget-prefix' --no-enable" \
        /dev/null > "$TEMP/wget-tty.log" 2>&1
    python3 - "$TEMP/wget-tty.log" <<'PY'
import pathlib
import re
import sys

output = pathlib.Path(sys.argv[1]).read_bytes()
frames = re.findall(rb"\[INFO\]: Downloading release archive (\.+)", output)
assert frames[:4] == [b".", b"..", b"...", b"."], frames
PY
    grep -F '[WARNING]: curl is unavailable; using wget.' "$TEMP/wget-tty.log" >/dev/null
    unset INSTALL_TEST_DELAY_ARCHIVE
fi

printf '%064d  dimmer-linux-x86_64.tar.gz\n' 0 > "$FIXTURES/SHA256SUMS"
if bash "$ROOT/scripts/install-github.sh" --prefix "$TEMP/rejected" >"$TEMP/output" 2>&1; then
    echo 'Installer accepted an archive with an invalid checksum.' >&2
    exit 1
fi
grep -F '[ERROR]:' "$TEMP/output" >/dev/null
[ ! -e "$TEMP/rejected/bin/dimmer" ] || {
    echo 'Installer wrote files after a checksum failure.' >&2
    exit 1
}

INSTALL_TEST_FAIL_DOWNLOAD=archive
export INSTALL_TEST_FAIL_DOWNLOAD
if bash "$ROOT/scripts/install-github.sh" --prefix "$TEMP/download-failed" >"$TEMP/output" 2>&1; then
    echo 'Installer accepted a failed archive download.' >&2
    exit 1
fi
grep -F '[ERROR]: Could not download the Dimmer release archive.' "$TEMP/output" >/dev/null

echo 'GitHub installer checks passed.'
