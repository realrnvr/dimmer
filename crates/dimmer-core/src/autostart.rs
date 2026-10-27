//! XDG autostart integration.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsutil::write_atomic;

/// Desktop-entry filename used for login autostart.
pub const AUTOSTART_FILE: &str = "io.github.dimmer.Linux.desktop";

/// Resolves `$XDG_CONFIG_HOME/autostart`, falling back to `~/.config/autostart`.
#[must_use]
pub fn autostart_dir(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let base = xdg_config_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))?;
    Some(base.join("autostart"))
}

/// Returns the full autostart desktop-entry path.
#[must_use]
pub fn autostart_path(directory: &Path) -> PathBuf {
    directory.join(AUTOSTART_FILE)
}

/// Escapes one `Exec=` argument according to the Desktop Entry specification.
#[must_use]
pub fn quote_exec_argument(argument: &str) -> String {
    let mut escaped = String::with_capacity(argument.len() + 2);
    escaped.push('"');
    for character in argument.chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                escaped.push('\\');
                escaped.push(character);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

/// Builds the full autostart desktop entry.
#[must_use]
pub fn desktop_entry(executable: &Path, start_minimised: bool) -> String {
    let executable = executable.to_string_lossy();
    let mut exec = quote_exec_argument(&executable);
    if start_minimised {
        exec.push_str(" --minimised");
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Dimmer\n\
         GenericName=Screen Dimmer\n\
         Comment=Dim every display independently\n\
         Exec={exec}\n\
         Icon=io.github.dimmer.Linux\n\
         Terminal=false\n\
         Categories=Utility;Accessibility;\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Installs or removes the autostart entry.  Returns the path that was written or
/// removed.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be written or removed.
pub fn set_enabled(
    directory: &Path,
    enabled: bool,
    executable: &Path,
    start_minimised: bool,
) -> io::Result<PathBuf> {
    let path = autostart_path(directory);
    if enabled {
        write_atomic(&path, desktop_entry(executable, start_minimised).as_bytes(), 0o644)?;
    } else if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(path)
}

/// Returns whether the autostart entry currently exists.
#[must_use]
pub fn is_enabled(directory: &Path) -> bool {
    autostart_path(directory).is_file()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::field_reassign_with_default
    )]

    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dimmer-autostart-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn xdg_directory_is_preferred() {
        let directory = autostart_dir(Some(Path::new("/tmp/xdg")), Some(Path::new("/home/u")));
        assert_eq!(directory, Some(PathBuf::from("/tmp/xdg/autostart")));
    }

    #[test]
    fn home_directory_is_fallback() {
        let directory = autostart_dir(None, Some(Path::new("/home/u")));
        assert_eq!(directory, Some(PathBuf::from("/home/u/.config/autostart")));
    }

    #[test]
    fn exec_quoting_handles_spaces_and_reserved_characters() {
        assert_eq!(quote_exec_argument("/usr/bin/dimmer"), "\"/usr/bin/dimmer\"");
        assert_eq!(quote_exec_argument("/home/a b/dimmer"), "\"/home/a b/dimmer\"");
        assert_eq!(quote_exec_argument("/tmp/$x`y\\z"), "\"/tmp/\\$x\\`y\\\\z\"");
        assert_eq!(quote_exec_argument("/tmp/100%"), "\"/tmp/100%%\"");
    }

    #[test]
    fn desktop_entry_has_required_keys() {
        let entry = desktop_entry(Path::new("/home/u/.local/bin/dimmer"), true);
        assert!(entry.starts_with("[Desktop Entry]"));
        assert!(entry.contains("Exec=\"/home/u/.local/bin/dimmer\" --minimised"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
        assert!(entry.contains("Icon=io.github.dimmer.Linux"));
        assert!(!entry.contains("Terminal=true"));
    }

    #[test]
    fn enable_disable_is_idempotent() {
        let directory = temp_dir("toggle");
        let executable = Path::new("/usr/bin/dimmer");
        assert!(!is_enabled(&directory));
        let path = set_enabled(&directory, true, executable, false).unwrap();
        assert!(path.exists());
        assert!(is_enabled(&directory));
        set_enabled(&directory, true, executable, true).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("--minimised"));
        set_enabled(&directory, false, executable, false).unwrap();
        assert!(!path.exists());
        set_enabled(&directory, false, executable, false).unwrap();
        assert!(!is_enabled(&directory));
        fs::remove_dir_all(directory).unwrap();
    }
}
