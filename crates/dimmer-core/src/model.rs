//! Persistent settings, matching the original `Dimmer.ini` JSON document.

use serde::{Deserialize, Serialize};

use crate::dim::{clamp_dim, normalize_tint};
use crate::time::now_string;

/// Saved window position.  The original uses nullable doubles in logical pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct WindowPosition {
    pub left: Option<f64>,
    pub top: Option<f64>,
}

impl Default for WindowPosition {
    fn default() -> Self {
        Self { left: Some(0.0), top: Some(0.0) }
    }
}

/// Windows autostart metadata.  `Mode` is kept for JSON compatibility; Linux
/// uses XDG autostart instead of startup-folder shortcuts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AutoRun {
    pub on: bool,
    pub all_users: bool,
    pub mode: String,
}

impl Default for AutoRun {
    fn default() -> Self {
        Self { on: false, all_users: false, mode: "StartupFolder".to_owned() }
    }
}

/// Master dimmer state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct MasterSettings {
    pub on: bool,
    pub dim: f64,
}

impl Default for MasterSettings {
    fn default() -> Self {
        Self { on: false, dim: 30.0 }
    }
}

/// Per-screen state.  `Connector` is a Linux extension; the original application
/// ignores unknown JSON members.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ScreenSettings {
    pub id: i32,
    pub on: bool,
    pub dim: f64,
    pub tint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connector: Option<String>,
}

impl Default for ScreenSettings {
    fn default() -> Self {
        Self { id: 1, on: true, dim: 30.0, tint: "000000".to_owned(), connector: None }
    }
}

/// Entire settings document.  Field names deliberately match Dimmer 2.0.1 so a
/// `Dimmer.ini` can be copied between platforms.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct AppSettings {
    pub position: WindowPosition,
    pub show_ids: bool,
    pub show_boundaries: bool,
    pub auto_run: AutoRun,
    pub movable_window: bool,
    pub start_minimised: bool,
    pub non_dimmable_window: bool,
    pub close_hides_to_tray: bool,
    pub show_in_taskbar: bool,
    pub master: MasterSettings,
    pub screens: Vec<ScreenSettings>,
    pub last_run: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            position: WindowPosition::default(),
            show_ids: false,
            show_boundaries: false,
            auto_run: AutoRun::default(),
            movable_window: true,
            start_minimised: false,
            non_dimmable_window: false,
            close_hides_to_tray: false,
            show_in_taskbar: true,
            master: MasterSettings::default(),
            screens: vec![ScreenSettings::default()],
            last_run: now_string(),
        }
    }
}

impl AppSettings {
    /// Clamps values loaded from disk and canonicalises colours.
    pub fn normalize(&mut self) {
        self.master.dim = clamp_dim(self.master.dim);
        if self.screens.is_empty() {
            self.screens.push(ScreenSettings::default());
        }
        for screen in &mut self.screens {
            screen.dim = clamp_dim(screen.dim);
            screen.tint = normalize_tint(&screen.tint).unwrap_or_else(|| "000000".to_owned());
            if screen.id < 0 {
                screen.id = 0;
            }
        }
    }

    /// Returns the screen for an index (screens are addressed by 1-based id).
    #[must_use]
    pub fn screen(&self, id: i32) -> Option<&ScreenSettings> {
        self.screens.iter().find(|screen| screen.id == id)
    }

    /// Mutable screen lookup by id.
    pub fn screen_mut(&mut self, id: i32) -> Option<&mut ScreenSettings> {
        self.screens.iter_mut().find(|screen| screen.id == id)
    }

    /// True when the master control can be used (the original disables it with
    /// one monitor).
    #[must_use]
    pub fn master_available(&self) -> bool {
        self.screens.iter().filter(|screen| screen.id > 0).count() > 1
    }

    /// Applies a master dim value to every active screen.
    pub fn apply_master_dim(&mut self, dim: f64) {
        let dim = clamp_dim(dim);
        self.master.dim = dim;
        for screen in &mut self.screens {
            if screen.id > 0 {
                screen.dim = dim;
            }
        }
    }

    /// Applies an external `SetMaster` request.
    ///
    /// Master is only valid with more than one monitor, matching the disabled
    /// master card in the original UI. Turning it off releases the lock but
    /// keeps the last applied per-screen values, as the original does.
    pub fn set_master(&mut self, on: bool, dim: f64) {
        let dim = clamp_dim(dim);
        self.master.dim = dim;
        if on && self.master_available() {
            self.master.on = true;
            self.apply_master_dim(dim);
        } else {
            self.master.on = false;
        }
    }

    /// Enables or disables every screen without losing its dim value.
    pub fn set_all_screens_on(&mut self, on: bool) {
        for screen in &mut self.screens {
            if screen.id > 0 {
                screen.on = on;
            }
        }
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
    use super::*;

    #[test]
    fn default_matches_original_values() {
        let settings = AppSettings::default();
        assert_eq!(settings.master.dim, 30.0);
        assert!(!settings.master.on);
        assert!(settings.movable_window);
        assert!(settings.show_in_taskbar);
        assert!(!settings.non_dimmable_window);
        assert_eq!(settings.screens.len(), 1);
        assert_eq!(settings.screens[0].tint, "000000");
        assert_eq!(settings.auto_run.mode, "StartupFolder");
    }

    #[test]
    fn parses_original_document() {
        let json = r#"{
            "Position": { "Left": 12.0, "Top": null },
            "ShowIds": true,
            "AutoRun": { "On": true, "AllUsers": false, "Mode": "StartupFolder" },
            "MovableWindow": false,
            "StartMinimised": true,
            "NonDimmableWindow": true,
            "CloseHidesToTray": true,
            "Master": { "On": true, "Dim": 55.0 },
            "Screens": [
                { "Id": 1, "On": false, "Dim": 12.0, "Tint": "FF0000" }
            ],
            "LastRun": "2022-12-01 10:00:00"
        }"#;
        let settings: AppSettings = serde_json::from_str(json).expect("valid original JSON");
        assert_eq!(settings.position.left, Some(12.0));
        assert_eq!(settings.position.top, None);
        assert!(settings.show_ids);
        assert!(!settings.show_boundaries);
        assert_eq!(settings.screens[0].tint, "FF0000");
        assert_eq!(settings.screens[0].connector, None);
        assert_eq!(settings.last_run, "2022-12-01 10:00:00");
    }

    #[test]
    fn missing_members_use_defaults() {
        let settings: AppSettings = serde_json::from_str("{}").expect("empty object");
        let defaults = AppSettings::default();
        assert_eq!(settings.position, defaults.position);
        assert_eq!(settings.master, defaults.master);
        assert_eq!(settings.screens, defaults.screens);
        assert_eq!(settings.movable_window, defaults.movable_window);
        assert_eq!(settings.auto_run, defaults.auto_run);
    }

    #[test]
    fn unknown_members_are_ignored() {
        let settings: AppSettings =
            serde_json::from_str(r#"{ "FutureOption": 42, "Screens": [] }"#)
                .expect("unknown member");
        assert_eq!(settings.screens.len(), 0);
    }

    #[test]
    fn normalize_clamps_and_canonicalises() {
        let mut settings = AppSettings::default();
        settings.master.dim = 200.0;
        settings.screens[0].dim = -4.0;
        settings.screens[0].tint = "#a0b1c2".to_owned();
        settings.normalize();
        assert_eq!(settings.master.dim, 90.0);
        assert_eq!(settings.screens[0].dim, 0.0);
        assert_eq!(settings.screens[0].tint, "A0B1C2");
    }

    #[test]
    fn master_applies_to_active_screens_only() {
        let mut settings = AppSettings::default();
        settings.screens.push(ScreenSettings { id: 2, ..ScreenSettings::default() });
        settings.screens.push(ScreenSettings { id: 0, dim: 5.0, ..ScreenSettings::default() });
        settings.apply_master_dim(44.0);
        assert_eq!(settings.screens[0].dim, 44.0);
        assert_eq!(settings.screens[1].dim, 44.0);
        assert_eq!(settings.screens[2].dim, 5.0);
    }

    #[test]
    fn set_master_is_ignored_with_one_screen() {
        let mut settings = AppSettings::default();
        let before = settings.screens[0].dim;
        settings.set_master(true, 55.0);
        assert!(!settings.master.on);
        assert_eq!(settings.master.dim, 55.0);
        assert_eq!(settings.screens[0].dim, before);
    }

    #[test]
    fn set_master_applies_and_retains_with_two_screens() {
        let mut settings = AppSettings::default();
        settings.screens.push(ScreenSettings { id: 2, ..ScreenSettings::default() });

        settings.set_master(true, 55.0);
        assert!(settings.master.on);
        assert_eq!(settings.screens[0].dim, 55.0);
        assert_eq!(settings.screens[1].dim, 55.0);

        settings.set_master(false, 30.0);
        assert!(!settings.master.on);
        assert_eq!(settings.master.dim, 30.0);
        assert_eq!(settings.screens[0].dim, 55.0);
        assert_eq!(settings.screens[1].dim, 55.0);
    }
}
