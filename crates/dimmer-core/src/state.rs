//! State snapshot published to the GNOME Shell extension over D-Bus.

use serde::{Deserialize, Serialize};

use crate::dim::{clamp_dim, effective_dim, normalize_tint};
use crate::model::{AppSettings, ScreenSettings};
use crate::monitor::MonitorInfo;

/// Protocol version understood by the extension.
pub const PROTOCOL_VERSION: u32 = 1;

/// Logical compositor coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Master state as published to the extension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MasterState {
    pub on: bool,
    pub dim: f64,
}

/// One connected screen with its effective (post-enable) dim value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScreenState {
    pub id: i32,
    pub on: bool,
    pub dim: f64,
    pub tint: String,
    pub connector: String,
    pub geometry: Geometry,
    pub scale: f64,
    pub primary: bool,
}

/// Complete overlay state document.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OverlayState {
    pub protocol: u32,
    pub app_version: String,
    pub extension_connected: bool,
    pub window_visible: bool,
    pub window_app_id: String,
    pub window_wm_class: String,
    pub window_reset_serial: u64,
    pub show_ids: bool,
    pub show_boundaries: bool,
    pub non_dimmable_window: bool,
    pub master: MasterState,
    pub screens: Vec<ScreenState>,
}

/// Runtime values not persisted in settings.
#[derive(Clone, Debug)]
pub struct StateContext {
    pub app_version: String,
    pub extension_connected: bool,
    pub window_visible: bool,
    pub window_app_id: String,
    pub window_wm_class: String,
    pub window_reset_serial: u64,
}

impl Default for StateContext {
    fn default() -> Self {
        Self {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            extension_connected: false,
            window_visible: false,
            window_app_id: "io.github.dimmer.Linux".to_owned(),
            window_wm_class: "dimmer".to_owned(),
            window_reset_serial: 0,
        }
    }
}

/// Builds a state snapshot.  `active` and `monitors` are expected to be aligned
/// by index; `sync_screens` produces `active` in that order.
#[must_use]
pub fn build_state(
    settings: &AppSettings,
    active: &[ScreenSettings],
    monitors: &[MonitorInfo],
    context: &StateContext,
) -> OverlayState {
    let screens = active
        .iter()
        .zip(monitors)
        .map(|(screen, monitor)| ScreenState {
            id: screen.id,
            on: screen.on,
            dim: effective_dim(screen.on, screen.dim),
            tint: normalize_tint(&screen.tint).unwrap_or_else(|| "000000".to_owned()),
            connector: monitor.connector.clone(),
            geometry: monitor.geometry,
            scale: monitor.scale,
            primary: monitor.primary,
        })
        .collect();

    OverlayState {
        protocol: PROTOCOL_VERSION,
        app_version: context.app_version.clone(),
        extension_connected: context.extension_connected,
        window_visible: context.window_visible,
        window_app_id: context.window_app_id.clone(),
        window_wm_class: context.window_wm_class.clone(),
        window_reset_serial: context.window_reset_serial,
        show_ids: settings.show_ids,
        show_boundaries: settings.show_boundaries,
        non_dimmable_window: settings.non_dimmable_window,
        master: MasterState { on: settings.master.on, dim: clamp_dim(settings.master.dim) },
        screens,
    }
}

impl OverlayState {
    /// Serialises the snapshot for the `GetState`/`StateChanged` D-Bus payload.
    ///
    /// # Errors
    ///
    /// Returns a serde error if a value cannot be represented as JSON (for
    /// example a non-finite number).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
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
    use crate::monitor::MonitorInfo;
    use crate::state::Geometry;
    use serde_json::Value;

    fn fixture() -> (AppSettings, Vec<ScreenSettings>, Vec<MonitorInfo>) {
        let mut settings = AppSettings::default();
        settings.show_ids = true;
        settings.master.dim = 40.0;
        let active = vec![
            ScreenSettings {
                id: 1,
                on: true,
                dim: 25.0,
                tint: "#ff8000".to_owned(),
                connector: None,
            },
            ScreenSettings { id: 2, on: false, dim: 60.0, tint: "bad".to_owned(), connector: None },
        ];
        let monitors = vec![
            MonitorInfo::new(
                "eDP-1",
                "Panel",
                Geometry { x: 0, y: 0, width: 1920, height: 1200 },
                1.5,
                true,
                0,
            ),
            MonitorInfo::new(
                "HDMI-1",
                "External",
                Geometry { x: 1920, y: 0, width: 2560, height: 1440 },
                1.0,
                false,
                1,
            ),
        ];
        (settings, active, monitors)
    }

    #[test]
    fn state_uses_effective_dims_and_canonical_tint() {
        let (settings, active, monitors) = fixture();
        let context = StateContext {
            app_version: "1.0.0".to_owned(),
            extension_connected: true,
            window_visible: true,
            ..StateContext::default()
        };
        let state = build_state(&settings, &active, &monitors, &context);
        assert_eq!(state.protocol, PROTOCOL_VERSION);
        assert_eq!(state.screens[0].dim, 25.0);
        assert_eq!(state.screens[0].tint, "FF8000");
        assert_eq!(state.screens[1].dim, 0.0);
        assert_eq!(state.screens[1].tint, "000000");
        assert_eq!(state.screens[1].geometry.width, 2560);
        assert!(state.screens[0].primary);
        assert!(!state.screens[1].primary);
    }

    #[test]
    fn json_uses_snake_case_and_protocol() {
        let (settings, active, monitors) = fixture();
        let state = build_state(&settings, &active, &monitors, &StateContext::default());
        let value: Value = serde_json::from_str(&state.to_json().expect("json")).expect("parse");
        assert_eq!(value["protocol"], 1);
        assert_eq!(value["show_ids"], true);
        assert_eq!(value["screens"][0]["connector"], "eDP-1");
        assert_eq!(value["master"]["dim"], 40.0);
        assert!(value.get("app_version").is_some());
    }

    #[test]
    fn empty_monitor_list_produces_no_screens() {
        let settings = AppSettings::default();
        let state = build_state(&settings, &[], &[], &StateContext::default());
        assert_eq!(state.screens, Vec::new());
    }
}
