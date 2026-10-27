//! Bundled GNOME Shell extension and its installer.
//!
//! The extension sources are embedded in the binary so `--install-extension`
//! works from an installed copy (release tarball, RPM, or a manually copied
//! binary) without needing the source tree.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::shared::{EXTENSION_UUID, extension_directory};

/// Files written by `--install-extension`, relative to the extension directory.
///
/// Keep this list in sync with `gnome-extension/dimmer@dimmer.linux/`.
const FILES: &[(&str, &str)] = &[
    ("metadata.json", include_str!("../../../gnome-extension/dimmer@dimmer.linux/metadata.json")),
    ("extension.js", include_str!("../../../gnome-extension/dimmer@dimmer.linux/extension.js")),
    ("dbusClient.js", include_str!("../../../gnome-extension/dimmer@dimmer.linux/dbusClient.js")),
    ("indicator.js", include_str!("../../../gnome-extension/dimmer@dimmer.linux/indicator.js")),
    (
        "overlayManager.js",
        include_str!("../../../gnome-extension/dimmer@dimmer.linux/overlayManager.js"),
    ),
    ("overlayMath.js", include_str!("../../../gnome-extension/dimmer@dimmer.linux/overlayMath.js")),
    ("windowReset.js", include_str!("../../../gnome-extension/dimmer@dimmer.linux/windowReset.js")),
    ("stylesheet.css", include_str!("../../../gnome-extension/dimmer@dimmer.linux/stylesheet.css")),
];

/// Installs the embedded extension into the current user's XDG data directory.
///
/// # Errors
///
/// Returns an error when the data directory cannot be determined or when any
/// file cannot be created.
pub fn install() -> Result<PathBuf, String> {
    let data_home =
        std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()).map(PathBuf::from);
    let home = std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from);

    if data_home.is_none() && home.is_none() {
        return Err(
            "neither XDG_DATA_HOME nor HOME is set; cannot locate the GNOME extensions directory"
                .to_owned(),
        );
    }

    let directory = extension_directory(data_home.as_deref(), home.as_deref());
    install_to(&directory).map_err(|error| {
        format!("could not install the extension to {}: {error}", directory.display())
    })?;
    Ok(directory)
}

fn install_to(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    for (name, contents) in FILES {
        fs::write(directory.join(name), contents)?;
    }
    Ok(())
}

/// Human-readable follow-up instructions after installing.
#[must_use]
pub fn instructions(directory: &Path) -> String {
    format!(
        "Installed the Dimmer GNOME extension in {}.\n\
         \n\
         GNOME Shell only loads newly installed extensions at login. Log out and back\n\
         in, then confirm it is enabled:\n\
         \n\
         \x20   gnome-extensions enable {EXTENSION_UUID}\n\
         \n\
         If Dimmer is already running, quit and start it again afterwards.",
        directory.display()
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temporary_directory() -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("dimmer-extension-test-{}-{stamp}", std::process::id()))
    }

    #[test]
    fn embedded_files_are_complete_and_installable() {
        let directory = temporary_directory();
        install_to(&directory).unwrap();

        for (name, contents) in FILES {
            let installed = fs::read_to_string(directory.join(name)).unwrap();
            assert_eq!(&installed, contents, "{name} differs from the embedded source");
            assert!(!installed.is_empty(), "{name} is empty");
        }

        let metadata = fs::read_to_string(directory.join("metadata.json")).unwrap();
        assert!(metadata.contains(EXTENSION_UUID));
        assert!(metadata.contains("\"50\""));

        // Installing again is idempotent.
        install_to(&directory).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), FILES.len());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn instructions_mention_login_and_enable() {
        let text = instructions(Path::new("/tmp/ext"));
        assert!(text.contains("Log out"));
        assert!(text.contains("gnome-extensions enable"));
        assert!(text.contains(EXTENSION_UUID));
    }
}
