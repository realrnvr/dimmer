#!/usr/bin/env bash
# Download and install the latest native Dimmer release for the current user.
set -euo pipefail

REPOSITORY=realrnvr/dimmer
ARCHIVE_NAME=dimmer-linux-x86_64.tar.gz
RELEASE_BASE=https://github.com/$REPOSITORY/releases/latest/download

die() {
    printf 'Dimmer installer: %s\n' "$1" >&2
    exit 1
}

: "${HOME:?HOME must be set to install Dimmer for the current user}"
command -v tar >/dev/null 2>&1 || die "tar is required to unpack the Dimmer release."
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required to verify the Dimmer release."

if command -v curl >/dev/null 2>&1; then
    download() { curl --fail --location --silent --show-error "$1" --output "$2"; }
elif command -v wget >/dev/null 2>&1; then
    download() { wget --quiet "$1" --output-document="$2"; }
else
    die "curl or wget is required to download Dimmer from GitHub."
fi

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT
archive=$temporary_directory/$ARCHIVE_NAME
checksum_file=$temporary_directory/SHA256SUMS
bundle=$temporary_directory/bundle

echo "Downloading the latest Dimmer release from GitHub…"
download "$RELEASE_BASE/$ARCHIVE_NAME" "$archive" \
    || die "Could not download the Dimmer release archive."
download "$RELEASE_BASE/SHA256SUMS" "$checksum_file" \
    || die "Could not download the Dimmer release checksum file."
grep -F "  $ARCHIVE_NAME" "$checksum_file" \
    | (cd "$temporary_directory" && sha256sum --check --status -) \
    || die "The Dimmer release archive checksum did not match."

mkdir -p "$bundle"
tar -tzf "$archive" | while IFS= read -r path; do
    case "$path" in
        /*|../*|*/../*|*/..)
            die "The release archive contains an unsafe path."
            ;;
    esac
done
tar --no-same-owner --no-same-permissions -xzf "$archive" -C "$bundle"
[ -x "$bundle/dimmer" ] || die "The release archive does not contain the Dimmer binary."
[ -x "$bundle/scripts/install.sh" ] || die "The release archive does not contain the installer."
[ -f "$bundle/data/io.github.dimmer.Linux.desktop" ] \
    || die "The release archive is missing the desktop launcher."
[ -f "$bundle/data/io.github.dimmer.Linux.metainfo.xml" ] \
    || die "The release archive is missing AppStream metadata."
[ -f "$bundle/data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" ] \
    || die "The release archive is missing the application icon."

"$bundle/scripts/install.sh" --binary "$bundle/dimmer" "$@"
