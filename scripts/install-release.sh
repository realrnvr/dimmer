#!/bin/sh
# Download and install the latest native Dimmer release for the current user.
# POSIX sh so the documented one-liner can pipe this into `sh`.
set -eu

REPOSITORY=realrnvr/dimmer
ARCHIVE_NAME=dimmer-linux-x86_64.tar.gz
RELEASE_BASE=https://github.com/$REPOSITORY/releases/latest/download

log_info() {
    printf '[INFO]: %s\n' "$1"
}

log_warning() {
    printf '[WARNING]: %s\n' "$1" >&2
}

die() {
    printf '[ERROR]: %s\n' "$1" >&2
    exit 1
}

[ -n "${HOME:-}" ] || die "HOME must be set to install Dimmer for the current user."
command -v tar >/dev/null 2>&1 || die "tar is required to unpack the Dimmer release."
command -v gzip >/dev/null 2>&1 || die "gzip is required to unpack the Dimmer release."
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required to verify the Dimmer release."

if command -v curl >/dev/null 2>&1; then
    DOWNLOAD_TOOL=curl
elif command -v wget >/dev/null 2>&1; then
    DOWNLOAD_TOOL=wget
    log_warning "curl is unavailable; using wget."
else
    die "curl or wget is required to download Dimmer from GitHub."
fi

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT
archive=$temporary_directory/$ARCHIVE_NAME
checksum_file=$temporary_directory/SHA256SUMS
checksum_entry=$temporary_directory/archive.sha256
bundle=$temporary_directory/bundle

download() {
    if [ "$DOWNLOAD_TOOL" = curl ]; then
        curl --fail --location --silent --show-error "$1" --output "$2"
    else
        wget --quiet "$1" --output-document="$2"
    fi
}

download_with_spinner() {
    download "$1" "$2" &
    download_pid=$!
    frame=1
    while kill -0 "$download_pid" 2>/dev/null; do
        case $frame in
            1) dots='.' ;;
            2) dots='..' ;;
            *) dots='...' ;;
        esac
        printf '\r[INFO]: Downloading release archive %-3s' "$dots" >&2
        frame=$((frame % 3 + 1))
        sleep 0.2
    done

    if wait "$download_pid"; then
        printf '\r[INFO]: Download complete.   \n' >&2
    else
        status=$?
        printf '\r%-48s\r' '' >&2
        return "$status"
    fi
}

if [ -t 2 ]; then
    download_with_spinner "$RELEASE_BASE/$ARCHIVE_NAME" "$archive" \
        || die "Could not download the Dimmer release archive."
else
    log_info "Downloading release archive."
    download "$RELEASE_BASE/$ARCHIVE_NAME" "$archive" \
        || die "Could not download the Dimmer release archive."
fi

log_info "Downloading release checksums."
download "$RELEASE_BASE/SHA256SUMS" "$checksum_file" \
    || die "Could not download the Dimmer release checksum file."

log_info "Verifying release archive checksum."
grep -F "  $ARCHIVE_NAME" "$checksum_file" > "$checksum_entry" \
    || die "The release checksum file does not list $ARCHIVE_NAME."
(cd "$temporary_directory" && sha256sum --check --status archive.sha256) \
    || die "The Dimmer release archive checksum did not match."

log_info "Checking release archive contents."
tar -tzf "$archive" > "$temporary_directory/archive-files" \
    || die "Could not read the release archive contents."
while IFS= read -r path; do
    case "$path" in
        /*|../*|*/../*|*/..)
            die "The release archive contains an unsafe path."
            ;;
    esac
done < "$temporary_directory/archive-files"

mkdir -p "$bundle" || die "Could not create a temporary extraction directory."
log_info "Extracting release archive."
tar --no-same-owner --no-same-permissions -xzf "$archive" -C "$bundle" \
    || die "Could not extract the release archive."

[ -x "$bundle/dimmer" ] || die "The release archive does not contain the Dimmer binary."
[ -x "$bundle/scripts/install.sh" ] || die "The release archive does not contain the installer."
[ -f "$bundle/data/io.github.dimmer.Linux.desktop" ] \
    || die "The release archive is missing the desktop launcher."
[ -f "$bundle/data/io.github.dimmer.Linux.metainfo.xml" ] \
    || die "The release archive is missing AppStream metadata."
[ -f "$bundle/data/icons/hicolor/scalable/apps/io.github.dimmer.Linux.svg" ] \
    || die "The release archive is missing the application icon."

log_info "Installing Dimmer."
"$bundle/scripts/install.sh" --binary "$bundle/dimmer" "$@" \
    || die "Dimmer installation failed."
