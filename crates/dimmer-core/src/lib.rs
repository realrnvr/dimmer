//! Pure, unit-testable model for Dimmer for Linux.
//!
//! This crate deliberately has no GTK or D-Bus dependency so settings, monitor
//! matching and dimming maths can be tested without a display server.

pub mod autostart;
pub mod diagnostics;
pub mod dim;
pub(crate) mod fsutil;
pub mod model;
pub mod monitor;
pub mod settings;
pub mod state;
pub mod time;

pub use diagnostics::DiagnosticReport;
pub use model::{AppSettings, AutoRun, MasterSettings, ScreenSettings, WindowPosition};
pub use monitor::{MonitorInfo, sort_monitors, sync_screens};
pub use settings::{
    LoadedSettings, SETTINGS_FILE, SettingsError, SettingsStore, resolve_settings_path,
};
pub use state::{
    Geometry, MasterState, OverlayState, PROTOCOL_VERSION, ScreenState, StateContext, build_state,
};

/// Application id shared by GTK, desktop files and the D-Bus service.
pub const APP_ID: &str = "io.github.dimmer.Linux";
/// Human-readable application name.
pub const APP_NAME: &str = "Dimmer";
/// Version of the Rust application.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
