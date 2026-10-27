//! Settings path resolution, resilient loading and atomic persistence.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsutil::write_atomic;
use crate::model::AppSettings;
use crate::time::now_string;

/// File name used by the original application and this port.
pub const SETTINGS_FILE: &str = "Dimmer.ini";

/// Result of loading settings, including a recoverable warning for the Debug page.
#[derive(Clone, Debug)]
pub struct LoadedSettings {
    pub settings: AppSettings,
    pub path: PathBuf,
    pub warning: Option<String>,
    pub backup: Option<PathBuf>,
}

/// Settings failures that should be surfaced, never silently ignored.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("could not determine a settings directory")]
    NoConfigDir,
    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("settings are not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl SettingsError {
    fn io(path: &Path, source: io::Error) -> Self {
        Self::Io { path: path.to_path_buf(), source }
    }
}

/// Chooses the settings file path.
///
/// Order: explicit CLI path, portable `Dimmer.ini` beside the executable (only
/// when it already exists and is writable), `$XDG_CONFIG_HOME/dimmer/Dimmer.ini`,
/// then `~/.config/dimmer/Dimmer.ini`.
#[must_use]
pub fn resolve_settings_path(
    cli: Option<&Path>,
    exe_dir: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(path) = cli {
        return path.to_path_buf();
    }

    if let Some(directory) = exe_dir {
        let portable = directory.join(SETTINGS_FILE);
        if portable.is_file() && is_writable(&portable) {
            return portable;
        }
    }

    let base = xdg_config_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")));

    base.map_or_else(
        || PathBuf::from(SETTINGS_FILE),
        |base| base.join("dimmer").join(SETTINGS_FILE),
    )
}

fn is_writable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| !metadata.permissions().readonly())
}

/// A settings file with atomic save and corruption recovery.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the document, creating or repairing it when necessary.
    ///
    /// A corrupt document is copied to `Dimmer.ini.bak-<timestamp>` before the
    /// default document is written, so user data is never silently discarded.
    #[must_use]
    pub fn load(&self) -> LoadedSettings {
        let settings = AppSettings::default();

        if !self.path.exists() {
            let warning = self.save(&settings).err().map(|error| error.to_string());
            return LoadedSettings { settings, path: self.path.clone(), warning, backup: None };
        }

        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) => {
                return LoadedSettings {
                    settings,
                    path: self.path.clone(),
                    warning: Some(SettingsError::io(&self.path, error).to_string()),
                    backup: None,
                };
            }
        };

        if text.trim().is_empty() {
            let backup = self.back_up().ok();
            let warning = self.save(&settings).err().map_or_else(
                || Some("settings file was empty; defaults restored".to_owned()),
                |error| Some(error.to_string()),
            );
            return LoadedSettings { settings, path: self.path.clone(), warning, backup };
        }

        match serde_json::from_str::<AppSettings>(&text) {
            Ok(mut parsed) => {
                parsed.normalize();
                if parsed.last_run.trim().is_empty() {
                    parsed.last_run = now_string();
                }
                LoadedSettings {
                    settings: parsed,
                    path: self.path.clone(),
                    warning: None,
                    backup: None,
                }
            }
            Err(error) => {
                let backup = self.back_up().ok();
                let mut warning = format!(
                    "settings file is invalid and was backed up ({error}); defaults restored"
                );
                if let Err(save_error) = self.save(&settings) {
                    warning =
                        format!("{warning}; additionally could not save defaults: {save_error}");
                }
                LoadedSettings { settings, path: self.path.clone(), warning: Some(warning), backup }
            }
        }
    }

    /// Saves settings atomically (temporary file, fsync, rename).
    ///
    /// # Errors
    ///
    /// Returns I/O or JSON errors without leaving a partially written document.
    pub fn save(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        let mut json = serde_json::to_string_pretty(settings)?;
        json.push('\n');
        write_atomic(&self.path, json.as_bytes(), 0o600)
            .map_err(|error| SettingsError::io(&self.path, error))
    }

    /// Copies the current file to a timestamped backup.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the file cannot be copied.
    pub fn back_up(&self) -> Result<PathBuf, SettingsError> {
        let timestamp = now_string().replace(' ', "_").replace(':', "");
        let mut backup = self.path.with_file_name(format!("{SETTINGS_FILE}.bak-{timestamp}"));
        let mut counter = 2_u32;
        while backup.exists() {
            backup = self.path.with_file_name(format!("{SETTINGS_FILE}.bak-{timestamp}-{counter}"));
            counter += 1;
        }
        fs::copy(&self.path, &backup).map_err(|error| SettingsError::io(&backup, error))?;
        Ok(backup)
    }
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
    use crate::model::ScreenSettings;

    fn temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dimmer-core-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn cli_path_wins() {
        let path = resolve_settings_path(
            Some(Path::new("/tmp/custom.ini")),
            Some(Path::new("/opt/dimmer")),
            Some(Path::new("/tmp/xdg")),
            Some(Path::new("/home/user")),
        );
        assert_eq!(path, Path::new("/tmp/custom.ini"));
    }

    #[test]
    fn portable_file_wins_when_writable() {
        let directory = temp_dir("portable");
        let portable = directory.join(SETTINGS_FILE);
        fs::write(&portable, "{}").unwrap();
        let path = resolve_settings_path(
            None,
            Some(&directory),
            Some(Path::new("/tmp/xdg")),
            Some(Path::new("/home/user")),
        );
        assert_eq!(path, portable);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn portable_file_is_skipped_when_absent() {
        let directory = temp_dir("portable-absent");
        let path = resolve_settings_path(
            None,
            Some(&directory),
            Some(Path::new("/tmp/xdg")),
            Some(Path::new("/home/user")),
        );
        assert_eq!(path, Path::new("/tmp/xdg/dimmer/Dimmer.ini"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn home_is_fallback_for_xdg() {
        let path = resolve_settings_path(None, None, None, Some(Path::new("/home/user")));
        assert_eq!(path, Path::new("/home/user/.config/dimmer/Dimmer.ini"));
    }

    #[test]
    fn save_and_load_round_trip() {
        let directory = temp_dir("round-trip");
        let store = SettingsStore::new(directory.join(SETTINGS_FILE));
        let mut settings = AppSettings::default();
        settings.show_boundaries = true;
        settings.screens[0] = ScreenSettings {
            id: 1,
            dim: 61.0,
            tint: "123456".to_owned(),
            ..ScreenSettings::default()
        };
        store.save(&settings).unwrap();

        let loaded = store.load();
        assert!(loaded.warning.is_none());
        assert!(loaded.settings.show_boundaries);
        assert_eq!(loaded.settings.screens[0].dim, 61.0);
        assert_eq!(loaded.settings.screens[0].tint, "123456");
        assert!(store.path().exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_file_is_created() {
        let directory = temp_dir("create");
        let store = SettingsStore::new(directory.join(SETTINGS_FILE));
        let loaded = store.load();
        assert!(loaded.warning.is_none());
        assert!(store.path().exists());
        assert_eq!(loaded.settings.screens[0].dim, 30.0);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let directory = temp_dir("corrupt");
        let path = directory.join(SETTINGS_FILE);
        fs::write(&path, "{ this is not json").unwrap();
        let store = SettingsStore::new(&path);
        let loaded = store.load();
        let warning = loaded.warning.unwrap();
        assert!(warning.contains("invalid"));
        let backup = loaded.backup.unwrap();
        assert!(backup.exists());
        assert_eq!(fs::read_to_string(backup).unwrap(), "{ this is not json");
        let repaired = fs::read_to_string(path).unwrap();
        assert!(repaired.contains("\"Master\""));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn empty_file_is_repaired() {
        let directory = temp_dir("empty");
        let path = directory.join(SETTINGS_FILE);
        fs::write(&path, "  \n").unwrap();
        let store = SettingsStore::new(&path);
        let loaded = store.load();
        assert!(loaded.warning.is_some());
        assert!(fs::read_to_string(path).unwrap().contains("\"Screens\""));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn values_are_clamped_on_load() {
        let directory = temp_dir("clamp");
        let path = directory.join(SETTINGS_FILE);
        fs::write(
            &path,
            r#"{"Master":{"On":true,"Dim":900},"Screens":[{"Id":1,"Dim":-2,"Tint":"xyz"}]}"#,
        )
        .unwrap();
        let store = SettingsStore::new(&path);
        let loaded = store.load();
        assert_eq!(loaded.settings.master.dim, 90.0);
        assert_eq!(loaded.settings.screens[0].dim, 0.0);
        assert_eq!(loaded.settings.screens[0].tint, "000000");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unwritable_directory_reports_error() {
        let directory = temp_dir("unwritable");
        let path = directory.join("blocked").join(SETTINGS_FILE);
        fs::write(directory.join("blocked"), "not a directory").unwrap();
        let store = SettingsStore::new(&path);
        let loaded = store.load();
        assert!(loaded.warning.is_some());
        fs::remove_dir_all(directory).unwrap();
    }
}
