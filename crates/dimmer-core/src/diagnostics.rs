//! Human-readable diagnostics for the Debug tab.

use std::fs;

use crate::state::PROTOCOL_VERSION;

/// Values collected by the GTK application.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticReport {
    pub app_version: String,
    pub os: String,
    pub session_type: String,
    pub desktop: String,
    pub shell_version: String,
    pub settings_path: String,
    pub settings_exists: bool,
    pub extension_connected: bool,
    pub window_visible: bool,
    pub monitors: Vec<String>,
    pub last_run: String,
    pub last_error: Option<String>,
    pub state_json: String,
}

impl DiagnosticReport {
    /// Renders a stable, copy-pasteable report.
    #[must_use]
    #[allow(clippy::format_push_string)]
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Dimmer for Linux\n");
        out.push_str("================\n\n");
        out.push_str(&format!("Version: {}\n", self.app_version));
        out.push_str(&format!("D-Bus protocol: {PROTOCOL_VERSION}\n"));
        out.push_str(&format!("OS: {}\n", self.os));
        out.push_str(&format!("Session: {} ({})\n", self.session_type, self.desktop));
        out.push_str(&format!("GNOME Shell: {}\n", self.shell_version));
        out.push_str(&format!(
            "GNOME extension: {}\n",
            if self.extension_connected { "connected" } else { "not connected" }
        ));
        out.push_str(&format!(
            "Main window: {}\n",
            if self.window_visible { "visible" } else { "hidden" }
        ));
        out.push_str(&format!("\nSettings: {}\n", self.settings_path));
        out.push_str(&format!(
            "Settings exists: {}\n",
            if self.settings_exists { "yes" } else { "no" }
        ));
        out.push_str(&format!("Last run: {}\n", self.last_run));

        out.push_str("\nMonitors\n--------\n");
        if self.monitors.is_empty() {
            out.push_str("(none reported)\n");
        } else {
            for monitor in &self.monitors {
                out.push_str(monitor);
                out.push('\n');
            }
        }

        out.push_str("\nOverlay state\n-------------\n");
        out.push_str(if self.state_json.is_empty() { "{}" } else { &self.state_json });
        out.push('\n');

        if let Some(error) = &self.last_error {
            out.push_str("\nLast error\n----------\n");
            out.push_str(error);
            out.push('\n');
        }
        out
    }
}

/// Reads `PRETTY_NAME` (or `NAME`/`ID`) from `/etc/os-release`.
#[must_use]
pub fn os_pretty_name() -> String {
    fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|contents| os_release_value(&contents).map(str::to_owned))
        .unwrap_or_else(|| "Linux (unknown distribution)".to_owned())
}

/// Extracts the display name from an `os-release` document.
#[must_use]
pub fn os_release_value(contents: &str) -> Option<&str> {
    let mut name = None;
    for line in contents.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(unquote(value));
        }
        if name.is_none() {
            if let Some(value) = line.strip_prefix("NAME=") {
                name = Some(unquote(value));
            }
        }
    }
    name
}

fn unquote(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'')
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
    fn parses_pretty_name() {
        let contents =
            "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 44 (Workstation Edition)\"\n";
        assert_eq!(os_release_value(contents), Some("Fedora Linux 44 (Workstation Edition)"));
    }

    #[test]
    fn falls_back_to_name() {
        let contents = "NAME=TestOS\nID=test\n";
        assert_eq!(os_release_value(contents), Some("TestOS"));
    }

    #[test]
    fn report_contains_sections() {
        let report = DiagnosticReport {
            app_version: "1.0.0".to_owned(),
            os: "Fedora Linux 44".to_owned(),
            session_type: "wayland".to_owned(),
            desktop: "GNOME".to_owned(),
            shell_version: "50.4".to_owned(),
            settings_path: "/home/u/.config/dimmer/Dimmer.ini".to_owned(),
            settings_exists: true,
            extension_connected: true,
            window_visible: false,
            monitors: vec!["eDP-1 1920x1200".to_owned()],
            last_run: "2026-10-25 12:00:00".to_owned(),
            last_error: Some("example error".to_owned()),
            state_json: "{\"protocol\":1}".to_owned(),
        };
        let rendered = report.render();
        assert!(rendered.contains("Dimmer for Linux"));
        assert!(rendered.contains("GNOME extension: connected"));
        assert!(rendered.contains("eDP-1 1920x1200"));
        assert!(rendered.contains("example error"));
        assert!(rendered.contains("{\"protocol\":1}"));
    }
}
