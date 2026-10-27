//! Monitor descriptors and reconciliation with saved per-screen settings.

use crate::model::{AppSettings, ScreenSettings};
use crate::state::Geometry;

/// A single Mutter/GDK monitor in logical (compositor) coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub connector: String,
    pub model: String,
    pub geometry: Geometry,
    pub scale: f64,
    pub primary: bool,
}

impl MonitorInfo {
    /// Creates a monitor descriptor, normalising missing connectors.
    #[must_use]
    pub fn new(
        connector: impl Into<String>,
        model: impl Into<String>,
        geometry: Geometry,
        scale: f64,
        primary: bool,
        index: usize,
    ) -> Self {
        let connector = connector.into();
        let connector = if connector.trim().is_empty() || connector == "Unknown" {
            format!("unknown-{}", index + 1)
        } else {
            connector
        };
        Self { connector, model: model.into(), geometry, scale, primary }
    }

    /// Human-readable name for diagnostics and cards.
    #[must_use]
    pub fn label(&self) -> String {
        if self.model.trim().is_empty() {
            self.connector.clone()
        } else {
            format!("{} ({})", self.model, self.connector)
        }
    }
}

/// Sorts monitors by position and connector, then rewrites screen ids to match.
pub fn sort_monitors(monitors: &mut [MonitorInfo]) {
    monitors.sort_by(|left, right| {
        left.geometry
            .x
            .cmp(&right.geometry.x)
            .then(left.geometry.y.cmp(&right.geometry.y))
            .then(left.connector.cmp(&right.connector))
    });
}

/// Matches saved per-screen settings to currently connected monitors.
///
/// Matching order is connector, then legacy position (id), then a fresh default.
/// Settings for disconnected monitors are kept with id `0` so reconnecting the
/// same connector restores the previous level.  The returned vector contains only
/// connected screens, in monitor order, with ids `1..=n`.
pub fn sync_screens(settings: &mut AppSettings, monitors: &[MonitorInfo]) -> Vec<ScreenSettings> {
    if monitors.is_empty() {
        return Vec::new();
    }

    let existing = settings.screens.clone();
    let mut claimed = vec![false; existing.len()];
    let mut active = Vec::with_capacity(monitors.len());

    for (index, monitor) in monitors.iter().enumerate() {
        let by_connector = existing.iter().enumerate().position(|(position, screen)| {
            !claimed[position] && screen.connector.as_deref() == Some(monitor.connector.as_str())
        });
        let by_id = existing.iter().enumerate().position(|(position, screen)| {
            !claimed[position]
                && screen.connector.is_none()
                && screen.id == i32::try_from(index + 1).unwrap_or(i32::MAX)
        });
        let found = by_connector.or(by_id);
        let mut screen = found.map_or_else(ScreenSettings::default, |position| {
            claimed[position] = true;
            existing[position].clone()
        });
        screen.id = i32::try_from(index + 1).unwrap_or(i32::MAX);
        screen.connector = Some(monitor.connector.clone());
        active.push(screen);
    }

    let orphaned: Vec<ScreenSettings> = existing
        .into_iter()
        .enumerate()
        .filter(|(position, screen)| !claimed[*position] && screen.connector.is_some())
        .map(|(_, mut screen)| {
            screen.id = 0;
            screen
        })
        .collect();

    settings.screens.clone_from(&active);
    settings.screens.extend(orphaned);
    active
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
    use crate::state::Geometry;

    fn monitor(connector: &str, x: i32, y: i32, primary: bool) -> MonitorInfo {
        MonitorInfo::new(
            connector,
            "Test Display",
            Geometry { x, y, width: 1920, height: 1080 },
            1.0,
            primary,
            0,
        )
    }

    #[test]
    fn sorts_by_position_then_connector() {
        let mut monitors = vec![
            monitor("HDMI-2", 1920, 0, false),
            monitor("eDP-1", 0, 0, true),
            monitor("DP-3", 0, 0, false),
        ];
        sort_monitors(&mut monitors);
        assert_eq!(
            monitors.iter().map(|monitor| monitor.connector.as_str()).collect::<Vec<_>>(),
            vec!["DP-3", "eDP-1", "HDMI-2"]
        );
    }

    #[test]
    fn missing_connector_gets_fallback() {
        let monitor = MonitorInfo::new("", "Built-in", Geometry::default(), 1.0, true, 0);
        assert_eq!(monitor.connector, "unknown-1");
        let monitor = MonitorInfo::new("Unknown", "Built-in", Geometry::default(), 1.0, true, 1);
        assert_eq!(monitor.connector, "unknown-2");
    }

    #[test]
    fn connector_identity_survives_reordering() {
        let mut settings = AppSettings::default();
        settings.screens[0] = ScreenSettings {
            id: 1,
            dim: 11.0,
            connector: Some("eDP-1".to_owned()),
            ..ScreenSettings::default()
        };
        settings.screens.push(ScreenSettings {
            id: 2,
            dim: 22.0,
            connector: Some("HDMI-1".to_owned()),
            ..ScreenSettings::default()
        });

        let monitors = vec![monitor("HDMI-1", 0, 0, true), monitor("eDP-1", 1920, 0, false)];
        let active = sync_screens(&mut settings, &monitors);
        assert_eq!(active[0].dim, 22.0);
        assert_eq!(active[0].id, 1);
        assert_eq!(active[1].dim, 11.0);
        assert_eq!(active[1].id, 2);
    }

    #[test]
    fn disconnected_settings_are_restored() {
        let mut settings = AppSettings::default();
        settings.screens[0] = ScreenSettings {
            id: 1,
            dim: 42.0,
            connector: Some("eDP-1".to_owned()),
            ..ScreenSettings::default()
        };

        let single = vec![monitor("HDMI-1", 0, 0, true)];
        let active = sync_screens(&mut settings, &single);
        assert_eq!(active[0].dim, 30.0);
        assert_eq!(settings.screens.len(), 2);
        assert_eq!(settings.screens[1].dim, 42.0);
        assert_eq!(settings.screens[1].id, 0);

        let both = vec![monitor("HDMI-1", 0, 0, true), monitor("eDP-1", 1920, 0, false)];
        let active = sync_screens(&mut settings, &both);
        assert_eq!(active[1].dim, 42.0);
        assert_eq!(active[1].id, 2);
    }

    #[test]
    fn legacy_screen_is_matched_by_id() {
        let mut settings = AppSettings::default();
        settings.screens[0].dim = 17.0;
        let monitors = vec![monitor("eDP-1", 0, 0, true)];
        let active = sync_screens(&mut settings, &monitors);
        assert_eq!(active[0].dim, 17.0);
        assert_eq!(active[0].connector.as_deref(), Some("eDP-1"));
    }

    #[test]
    fn no_monitors_leaves_settings_alone() {
        let mut settings = AppSettings::default();
        let before = settings.clone();
        let active = sync_screens(&mut settings, &[]);
        assert_eq!(active, Vec::new());
        assert_eq!(settings, before);
    }
}
