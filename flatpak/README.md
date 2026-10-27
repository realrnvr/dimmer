# Flatpak packaging

The manifest builds the app from the repository root and uses
GNOME Platform 50 to provide consistent GTK and libadwaita runtime libraries.
Cargo dependencies are downloaded from crates.io by Flatpak Builder using the
checksums in `Cargo.lock`; builds run offline after source preparation.

Regenerate the lockfile-derived Cargo sources after changing dependencies:

```sh
python3 scripts/generate-flatpak-cargo-sources.py
```

Build and install locally with Flatpak Builder:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean \
  --repo=flatpak/repo flatpak/build flatpak/io.github.dimmer.Linux.yml
```

The app is sandboxed and cannot write the extension into the host's GNOME
extension directory. The release installer installs the versioned extension ZIP
on the host. The manifest grants read-only access to that directory so Debug can
report whether it is installed.

For Flathub, keep this manifest and generated Cargo source list in the Flathub
packaging repository, include `flathub.json` at that repository's root, and
replace the local `type: dir` source with a pinned source archive from a tagged
release. Update the source URL and hash for each release. The Flathub submission
repository should use the source archive root as the module root.
