#!/usr/bin/env bash
# Install the Flathub app and its host-side GNOME Shell extension for this user.
set -euo pipefail

APP_ID=io.github.dimmer.Linux
EXTENSION_UUID=dimmer@dimmer.linux
EXTENSION_ASSET=dimmer@dimmer.linux.shell-extension.zip
FLATHUB_URL=https://dl.flathub.org/repo/flathub.flatpakrepo

die() {
    printf 'Dimmer installer: %s\n' "$1" >&2
    exit 1
}

command -v flatpak >/dev/null 2>&1 || die "Flatpak is required. Install it using your distribution's package manager, then rerun this script."
command -v gnome-extensions >/dev/null 2>&1 || die "gnome-extensions is required on the host. Install the GNOME Shell extension tools, then rerun this script."
command -v unzip >/dev/null 2>&1 || die "unzip is required to install the host GNOME Shell extension."
: "${HOME:?HOME must be set to install Dimmer for the current user}"

if command -v curl >/dev/null 2>&1; then
    download() { curl --fail --location --silent --show-error "$1" --output "$2"; }
elif command -v wget >/dev/null 2>&1; then
    download() { wget --quiet "$1" --output-document="$2"; }
else
    die "curl or wget is required to download the GNOME extension release asset."
fi

if ! flatpak remotes --user --columns=name | grep -Fxq flathub; then
    echo "Adding the Flathub remote for this user…"
    flatpak remote-add --user --if-not-exists flathub "$FLATHUB_URL"
fi

echo "Installing or updating Dimmer from Flathub…"
flatpak install --user --assumeyes flathub "$APP_ID"

version_output=$(flatpak run --user --command=dimmer "$APP_ID" --version) \
    || die "Could not read the installed Flatpak app version."
case "$version_output" in
    "Dimmer "*) app_version=${version_output#Dimmer } ;;
    *) die "Unexpected version output from the installed Flatpak app: $version_output" ;;
esac
[[ "$app_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] \
    || die "The installed Flatpak reports an invalid release version: $app_version"
release_tag=v$app_version
release_url="https://github.com/realrnvr/dimmer/releases/download/$release_tag"

temporary_directory=$(mktemp -d)
trap 'rm -rf "$temporary_directory"' EXIT
extension_zip=$temporary_directory/$EXTENSION_ASSET
checksum_file=$temporary_directory/SHA256SUMS
extension_stage=$temporary_directory/extension
extension_data_home=${XDG_DATA_HOME:-$HOME/.local/share}
extension_directory=$extension_data_home/gnome-shell/extensions/$EXTENSION_UUID

echo "Downloading the GNOME Shell extension for Dimmer $app_version…"
download "$release_url/$EXTENSION_ASSET" "$extension_zip" \
    || die "Could not download the extension ZIP for release $release_tag."
download "$release_url/SHA256SUMS" "$checksum_file" \
    || die "Could not download the checksum file for release $release_tag."
grep -F "  $EXTENSION_ASSET" "$checksum_file" | (cd "$temporary_directory" && sha256sum --check --status -) \
    || die "The extension ZIP checksum did not match the published release checksum."
expected_files=$(printf '%s\n' dbusClient.js extension.js indicator.js metadata.json overlayManager.js overlayMath.js stylesheet.css | LC_ALL=C sort)
actual_files=$(unzip -Z1 "$extension_zip" | sed '/\/$/d' | LC_ALL=C sort)
[ "$actual_files" = "$expected_files" ] || die "The extension archive has an unexpected file list."
mkdir -p "$extension_stage"
unzip -q "$extension_zip" -d "$extension_stage"

echo "Installing the extension for this user…"
mkdir -p "$extension_data_home/gnome-shell/extensions"
rm -rf "$extension_directory"
mv "$extension_stage" "$extension_directory"

cat <<EOF

Dimmer is installed for this user.

GNOME Shell loads a newly installed extension after you log out and back in.
After logging back in, enable it if needed:
  gnome-extensions enable $EXTENSION_UUID

Launch Dimmer from the app grid or with:
  flatpak run $APP_ID

The Flatpak app cannot install the extension into the host by itself. Rerun this
script after an app release to update the host extension as well.
EOF
