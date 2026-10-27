//! Thread-safe application state shared by GTK, D-Bus and the diagnostics page.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dimmer_core::autostart;
use dimmer_core::diagnostics::DiagnosticReport;
use dimmer_core::dim::clamp_dim;
use dimmer_core::model::{AppSettings, ScreenSettings};
use dimmer_core::monitor::{MonitorInfo, sort_monitors, sync_screens};
use dimmer_core::settings::LoadedSettings;
use dimmer_core::state::{StateContext, build_state};
use dimmer_core::time::now_string;
use gtk::glib;
use tokio::sync::mpsc::UnboundedSender;

use crate::log::LogBuffer;

/// Well-known D-Bus name owned by the application.
pub const DBUS_NAME: &str = "io.github.dimmer.Linux";
/// Object path exported by the application.
pub const DBUS_PATH: &str = "/io/github/dimmer/Linux";
/// Interface implemented by the application.
pub const DBUS_INTERFACE: &str = "io.github.dimmer.Linux";
/// Extension UUID used in the GNOME extensions data directory.
pub const EXTENSION_UUID: &str = "dimmer@dimmer.linux";
/// Window title / WM class hint used by the extension for the non-dimmable hole.
pub const WINDOW_WM_CLASS: &str = "dimmer";

/// Commands sent from D-Bus to the GTK main loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCommand {
    ShowWindow,
    HideWindow,
    ResetWindowPosition,
    WindowResetCompleted { serial: u64, success: bool },
    Refresh,
    Quit,
}

/// On/off options exposed by the Options page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleOption {
    ShowIds,
    ShowBoundaries,
    NonDimmableWindow,
    MovableWindow,
    StartMinimised,
    CloseHidesToTray,
    ShowInTaskbar,
}

/// A consistent snapshot for rendering the UI.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub settings: AppSettings,
    pub active: Vec<ScreenSettings>,
    pub monitors: Vec<MonitorInfo>,
    pub extension_connected: bool,
    pub extension_version: Option<String>,
    pub window_visible: bool,
    pub settings_path: PathBuf,
    pub settings_warning: Option<String>,
    pub last_error: Option<String>,
    pub extension_installed: bool,
}

/// Environment details captured once at startup.
#[derive(Clone, Debug, Default)]
pub struct SessionInfo {
    pub os: String,
    pub session_type: String,
    pub desktop: String,
    pub shell_version: String,
}

struct SignalMessage {
    state_json: String,
    visibility: Option<bool>,
}

/// Application state shared across threads.
pub struct SharedState {
    settings_path: PathBuf,
    store: dimmer_core::SettingsStore,
    settings: Mutex<AppSettings>,
    monitors: Mutex<Vec<MonitorInfo>>,
    session: SessionInfo,
    window_visible: AtomicBool,
    window_reset_serial: AtomicU64,
    extension_connected: AtomicBool,
    extension_version: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    settings_warning: Mutex<Option<String>>,
    ui_sender: Mutex<Option<UnboundedSender<UiCommand>>>,
    pending_ui: Mutex<Vec<UiCommand>>,
    signal_sender: Mutex<Option<tokio::sync::mpsc::UnboundedSender<SignalMessage>>>,
    runtime: tokio::runtime::Handle,
    auto_run_dir: Option<PathBuf>,
    extension_dir: PathBuf,
    log: Arc<LogBuffer>,
}

impl SharedState {
    /// Builds shared state from loaded settings.
    #[must_use]
    pub fn new(
        store: dimmer_core::SettingsStore,
        loaded: LoadedSettings,
        runtime: tokio::runtime::Handle,
        session: SessionInfo,
        verbose: bool,
    ) -> Arc<Self> {
        let log = Arc::new(LogBuffer::new(verbose));
        if let Some(warning) = &loaded.warning {
            log.warn(warning);
        }
        log.info(format!(
            "settings file: {} ({})",
            store.path().display(),
            if loaded.settings.screens.is_empty() { "empty" } else { "loaded" }
        ));

        let is_flatpak = std::env::var_os("FLATPAK_ID").is_some();
        let auto_run_dir = if is_flatpak {
            None
        } else {
            autostart::autostart_dir(
                env_path("XDG_CONFIG_HOME").as_deref(),
                env_path("HOME").as_deref(),
            )
        };
        let mut settings = loaded.settings;
        if !is_flatpak {
            if let Some(directory) = &auto_run_dir {
                let present = autostart::is_enabled(directory);
                if settings.auto_run.on != present {
                    log.info(format!(
                        "autostart entry {} (reconciled)",
                        if present { "present" } else { "missing" }
                    ));
                    settings.auto_run.on = present;
                }
            }
        }

        let extension_data_home = if is_flatpak {
            env_path("HOST_XDG_DATA_HOME").or_else(|| env_path("XDG_DATA_HOME"))
        } else {
            env_path("XDG_DATA_HOME")
        };
        let extension_dir =
            extension_directory(extension_data_home.as_deref(), env_path("HOME").as_deref());

        Arc::new(Self {
            settings_path: store.path().to_path_buf(),
            store,
            settings: Mutex::new(settings),
            monitors: Mutex::new(Vec::new()),
            session,
            window_visible: AtomicBool::new(false),
            window_reset_serial: AtomicU64::new(0),
            extension_connected: AtomicBool::new(false),
            extension_version: Mutex::new(None),
            last_error: Mutex::new(None),
            settings_warning: Mutex::new(loaded.warning),
            ui_sender: Mutex::new(None),
            pending_ui: Mutex::new(Vec::new()),
            signal_sender: Mutex::new(None),
            runtime,
            auto_run_dir,
            extension_dir,
            log,
        })
    }

    #[must_use]
    pub const fn log(&self) -> &Arc<LogBuffer> {
        &self.log
    }

    #[must_use]
    pub const fn session(&self) -> &SessionInfo {
        &self.session
    }

    #[must_use]
    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    #[must_use]
    pub fn extension_dir(&self) -> &Path {
        &self.extension_dir
    }

    #[must_use]
    pub fn extension_installed(&self) -> bool {
        self.extension_dir.join("metadata.json").is_file()
            && self.extension_dir.join("extension.js").is_file()
    }

    #[must_use]
    pub fn autostart_path(&self) -> Option<PathBuf> {
        self.auto_run_dir.as_deref().map(autostart::autostart_path)
    }

    #[must_use]
    pub fn settings_warning(&self) -> Option<String> {
        self.settings_warning.lock().ok().and_then(|warning| warning.clone())
    }

    #[must_use]
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|error| error.clone())
    }

    /// Associates the GTK command channel, delivering anything queued earlier.
    pub fn attach_ui(&self, sender: &UnboundedSender<UiCommand>) {
        if let Ok(mut slot) = self.ui_sender.lock() {
            *slot = Some(sender.clone());
        }
        let pending = self
            .pending_ui
            .lock()
            .map_or_else(|_| Vec::new(), |mut queue| std::mem::take(&mut *queue));
        for command in pending {
            let _ = sender.send(command);
        }
    }

    /// Sends a command to the GTK loop, or queues it until the UI exists.
    pub fn send_ui(&self, command: UiCommand) {
        let sender = self.ui_sender.lock().ok().and_then(|slot| slot.clone());
        if let Some(sender) = sender {
            let _ = sender.send(command);
        } else if let Ok(mut queue) = self.pending_ui.lock() {
            queue.push(command);
        }
    }

    /// Connects the signal emitter.  Safe to call once after the bus name is owned.
    pub fn attach_connection(self: &Arc<Self>, connection: zbus::Connection) {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<SignalMessage>();
        let log = Arc::clone(&self.log);
        self.runtime.spawn(async move {
            while let Some(mut message) = receiver.recv().await {
                tokio::time::sleep(Duration::from_millis(30)).await;
                while let Ok(next) = receiver.try_recv() {
                    message = next;
                }
                if let Err(error) = connection
                    .emit_signal(
                        None::<&str>,
                        DBUS_PATH,
                        DBUS_INTERFACE,
                        "StateChanged",
                        &(message.state_json.as_str(),),
                    )
                    .await
                {
                    log.warn(format!("could not emit StateChanged: {error}"));
                }
                if let Some(visible) = message.visibility {
                    if let Err(error) = connection
                        .emit_signal(
                            None::<&str>,
                            DBUS_PATH,
                            DBUS_INTERFACE,
                            "VisibilityChanged",
                            &(visible,),
                        )
                        .await
                    {
                        log.warn(format!("could not emit VisibilityChanged: {error}"));
                    }
                }
            }
        });
        if let Ok(mut slot) = self.signal_sender.lock() {
            *slot = Some(sender);
        }
        self.bump(None);
    }

    fn bump(&self, visibility: Option<bool>) {
        let sender = self.signal_sender.lock().ok().and_then(|slot| slot.clone());
        if let Some(sender) = sender {
            let message = SignalMessage { state_json: self.state_json(), visibility };
            let _ = sender.send(message);
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let settings = self
            .settings
            .lock()
            .map_or_else(|_| AppSettings::default(), |settings| settings.clone());
        let monitors =
            self.monitors.lock().map_or_else(|_| Vec::new(), |monitors| monitors.clone());
        let active = active_screens(&settings, monitors.len());
        Snapshot {
            settings,
            active,
            monitors,
            extension_connected: self.extension_connected.load(Ordering::SeqCst),
            extension_version: self
                .extension_version
                .lock()
                .ok()
                .and_then(|version| version.clone()),
            window_visible: self.window_visible.load(Ordering::SeqCst),
            settings_path: self.settings_path.clone(),
            settings_warning: self.settings_warning(),
            last_error: self.last_error(),
            extension_installed: self.extension_installed(),
        }
    }

    #[must_use]
    pub fn state_json(&self) -> String {
        let settings = self
            .settings
            .lock()
            .map_or_else(|_| AppSettings::default(), |settings| settings.clone());
        let monitors =
            self.monitors.lock().map_or_else(|_| Vec::new(), |monitors| monitors.clone());
        let active = active_screens(&settings, monitors.len());
        let context = StateContext {
            app_version: crate::APP_VERSION.to_owned(),
            extension_connected: self.extension_connected.load(Ordering::SeqCst),
            window_visible: self.window_visible.load(Ordering::SeqCst),
            window_app_id: dimmer_core::APP_ID.to_owned(),
            window_wm_class: WINDOW_WM_CLASS.to_owned(),
            window_reset_serial: self.window_reset_serial.load(Ordering::SeqCst),
        };
        let state = build_state(&settings, &active, &monitors, &context);
        state.to_json().unwrap_or_else(|error| {
            self.log.error(format!("could not serialise overlay state: {error}"));
            "{}".to_owned()
        })
    }

    #[must_use]
    pub fn settings_json(&self) -> String {
        self.settings
            .lock()
            .ok()
            .and_then(|settings| serde_json::to_string_pretty(&*settings).ok())
            .unwrap_or_else(|| "{}".to_owned())
    }

    /// Persists the current settings, updating `LastRun`.
    ///
    /// # Errors
    ///
    /// Returns the error message and records it for the Debug page.
    pub fn save(&self) -> Result<(), String> {
        let mut settings = self
            .settings
            .lock()
            .map_or_else(|_| AppSettings::default(), |settings| settings.clone());
        settings.last_run = local_timestamp();
        self.store.save(&settings).map_err(|error| {
            let message = error.to_string();
            self.record_error(&message);
            message
        })?;
        if let Ok(mut current) = self.settings.lock() {
            current.last_run = settings.last_run;
        }
        Ok(())
    }

    /// Replaces the monitor list, reconciling per-screen settings.
    pub fn set_monitors(&self, monitors: Vec<MonitorInfo>) {
        let mut sorted = monitors;
        sort_monitors(&mut sorted);
        {
            let Ok(current) = self.monitors.lock() else {
                return;
            };
            if *current == sorted {
                return;
            }
        }
        {
            let Ok(mut settings) = self.settings.lock() else {
                return;
            };
            sync_screens(&mut settings, &sorted);
            self.log.info(format!(
                "monitors: {} connected [{}]",
                sorted.len(),
                sorted
                    .iter()
                    .map(|monitor| monitor.connector.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if let Ok(mut current) = self.monitors.lock() {
            *current = sorted;
        }
        if let Err(error) = self.save() {
            self.log.warn(format!("could not save monitor layout: {error}"));
        }
        self.bump(None);
        self.send_ui(UiCommand::Refresh);
    }

    pub fn set_window_visible(&self, visible: bool) {
        if self.window_visible.swap(visible, Ordering::SeqCst) == visible {
            return;
        }
        self.log.info(format!("main window {}", if visible { "shown" } else { "hidden" }));
        self.bump(Some(visible));
    }

    /// Records an extension registration and returns the current state JSON.
    pub fn register_extension(&self, version: &str) -> String {
        let first = !self.extension_connected.swap(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.extension_version.lock() {
            *slot = Some(version.to_owned());
        }
        if first {
            self.log.info(format!("GNOME extension connected (version {version})"));
        }
        self.bump(None);
        self.send_ui(UiCommand::Refresh);
        self.state_json()
    }

    /// Clears the extension registration when the extension is disabled.
    pub fn unregister_extension(&self) {
        if !self.extension_connected.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Ok(mut slot) = self.extension_version.lock() {
            *slot = None;
        }
        self.log.info("GNOME extension disconnected");
        self.bump(None);
        self.send_ui(UiCommand::Refresh);
    }

    /// Acknowledges the extension's `Ping` without changing state.
    #[must_use]
    pub fn ping() -> String {
        format!("dimmer {} protocol={}", crate::APP_VERSION, dimmer_core::PROTOCOL_VERSION)
    }

    /// Replaces the last error shown on the Debug page.
    pub fn record_error(&self, message: &str) {
        self.log.error(message);
        if let Ok(mut error) = self.last_error.lock() {
            *error = Some(message.to_owned());
        }
    }

    /// Stores a non-fatal bus error.
    pub fn record_bus_error(&self, message: &str) {
        self.log.warn(message);
        if let Ok(mut error) = self.last_error.lock() {
            *error = Some(message.to_owned());
        }
    }

    /// Applies a UI change and notifies the extension.
    pub fn ui_mutate(&self, mutate: impl FnOnce(&mut AppSettings)) {
        if let Ok(mut settings) = self.settings.lock() {
            mutate(&mut settings);
        }
        self.bump(None);
        self.send_ui(UiCommand::Refresh);
    }

    pub fn ui_set_screen_on(&self, id: i32, on: bool) {
        self.ui_mutate(|settings| {
            if let Some(screen) = settings.screen_mut(id) {
                screen.on = on;
            }
        });
    }

    pub fn ui_set_screen_dim(&self, id: i32, dim: f64) {
        self.ui_mutate(|settings| {
            if let Some(screen) = settings.screen_mut(id) {
                screen.dim = clamp_dim(dim);
            }
        });
    }

    pub fn ui_set_screen_tint(&self, id: i32, tint: &str) {
        let tint = dimmer_core::dim::normalize_tint(tint).unwrap_or_else(|| "000000".to_owned());
        self.ui_mutate(|settings| {
            if let Some(screen) = settings.screen_mut(id) {
                screen.tint = tint;
            }
        });
    }

    pub fn ui_set_master_on(&self, on: bool) {
        self.ui_mutate(|settings| {
            if on && settings.master_available() {
                settings.master.on = true;
                settings.apply_master_dim(settings.master.dim);
            } else {
                settings.master.on = false;
            }
        });
    }

    pub fn ui_set_master_dim(&self, dim: f64) {
        self.ui_mutate(|settings| {
            if settings.master.on {
                settings.apply_master_dim(dim);
            } else {
                settings.master.dim = clamp_dim(dim);
            }
        });
    }

    /// Applies `SetMaster` from the indicator/extension and persists immediately.
    pub fn api_set_master(&self, on: bool, dim: f64) {
        self.ui_mutate(|settings| settings.set_master(on, dim));
        if let Err(error) = self.save() {
            self.log.warn(format!("could not save SetMaster: {error}"));
        }
    }

    /// Applies `SetScreen` from the indicator/extension and persists immediately.
    pub fn api_set_screen(&self, id: i32, on: bool, dim: f64) {
        let dim = clamp_dim(dim);
        self.ui_mutate(|settings| {
            if let Some(screen) = settings.screen_mut(id) {
                screen.on = on;
                screen.dim = dim;
            }
        });
        if let Err(error) = self.save() {
            self.log.warn(format!("could not save SetScreen: {error}"));
        }
    }

    /// Changes an Options-page toggle.  Autostart changes are written immediately.
    ///
    /// # Errors
    ///
    /// Returns an error when the autostart entry cannot be written.
    pub fn set_toggle(&self, option: ToggleOption, on: bool) -> Result<(), String> {
        self.ui_mutate(|settings| match option {
            ToggleOption::ShowIds => settings.show_ids = on,
            ToggleOption::ShowBoundaries => settings.show_boundaries = on,
            ToggleOption::NonDimmableWindow => settings.non_dimmable_window = on,
            ToggleOption::MovableWindow => settings.movable_window = on,
            ToggleOption::StartMinimised => settings.start_minimised = on,
            ToggleOption::CloseHidesToTray => settings.close_hides_to_tray = on,
            ToggleOption::ShowInTaskbar => settings.show_in_taskbar = on,
        });
        if option == ToggleOption::StartMinimised
            && self.settings.lock().is_ok_and(|settings| settings.auto_run.on)
        {
            self.write_autostart()?;
        }
        self.save()?;
        Ok(())
    }

    /// Installs or removes the XDG autostart entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the autostart entry cannot be written.
    pub fn set_autostart(&self, enabled: bool) -> Result<(), String> {
        if std::env::var_os("FLATPAK_ID").is_some() {
            return Err(
                "Autostart is not available in the Flatpak build. Use your desktop's startup applications settings instead."
                    .to_owned(),
            );
        }
        self.write_autostart_entry(enabled)?;
        self.ui_mutate(|settings| settings.auto_run.on = enabled);
        self.save()?;
        Ok(())
    }

    fn write_autostart(&self) -> Result<(), String> {
        let enabled = self.settings.lock().is_ok_and(|settings| settings.auto_run.on);
        self.write_autostart_entry(enabled)
    }

    fn write_autostart_entry(&self, enabled: bool) -> Result<(), String> {
        let directory = self.auto_run_dir.as_deref().ok_or_else(|| {
            "cannot determine the XDG autostart directory (HOME is not set)".to_owned()
        })?;
        let start_minimised = self.settings.lock().is_ok_and(|settings| settings.start_minimised);
        let executable = std::env::current_exe().map_err(|error| {
            let message = format!("cannot determine executable path: {error}");
            self.record_error(&message);
            message
        })?;
        autostart::set_enabled(directory, enabled, &executable, start_minimised).map_err(
            |error| {
                let message = format!("could not update autostart: {error}");
                self.record_error(&message);
                message
            },
        )?;
        self.log.info(format!(
            "autostart {} ({})",
            if enabled { "enabled" } else { "disabled" },
            executable.display()
        ));
        Ok(())
    }

    /// Requests the GNOME Shell extension to restore the window's default size and position.
    pub fn request_window_reset(&self) -> u64 {
        if let Ok(mut settings) = self.settings.lock() {
            settings.position.left = Some(0.0);
            settings.position.top = Some(0.0);
        }
        let serial = self.window_reset_serial.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        self.bump(None);
        self.send_ui(UiCommand::Refresh);
        serial
    }

    /// Builds the diagnostics report for the Debug page.
    #[must_use]
    #[allow(clippy::format_push_string)]
    pub fn diagnostics(&self) -> String {
        let snapshot = self.snapshot();
        let report = DiagnosticReport {
            app_version: crate::APP_VERSION.to_owned(),
            os: self.session.os.clone(),
            session_type: self.session.session_type.clone(),
            desktop: self.session.desktop.clone(),
            shell_version: self.session.shell_version.clone(),
            settings_path: snapshot.settings_path.display().to_string(),
            settings_exists: snapshot.settings_path.is_file(),
            extension_connected: snapshot.extension_connected,
            window_visible: snapshot.window_visible,
            monitors: snapshot
                .monitors
                .iter()
                .enumerate()
                .map(|(index, monitor)| {
                    format!(
                        "#{} {} — {}x{}+{}+{} @{}{}",
                        index + 1,
                        monitor.label(),
                        monitor.geometry.width,
                        monitor.geometry.height,
                        monitor.geometry.x,
                        monitor.geometry.y,
                        monitor.scale,
                        if monitor.primary { " (primary)" } else { "" }
                    )
                })
                .collect(),
            last_run: snapshot.settings.last_run.clone(),
            last_error: snapshot.last_error.clone().or(snapshot.settings_warning),
            state_json: self.state_json(),
        };
        let mut text = report.render();
        text.push_str(&format!(
            "\nExtension directory: {}\nExtension installed: {}\nExtension version: {}\n",
            self.extension_dir.display(),
            if snapshot.extension_installed { "yes" } else { "no" },
            snapshot.extension_version.as_deref().unwrap_or("not connected")
        ));
        if let Some(path) = self.autostart_path() {
            text.push_str(&format!(
                "Autostart file: {} ({})\n",
                path.display(),
                if path.is_file() { "present" } else { "absent" }
            ));
        }
        text.push_str(&format!("\nSettings JSON\n-------------\n{}\n", self.settings_json()));
        text.push_str("\nLog\n---\n");
        text.push_str(&self.log.text());
        text.push('\n');
        text
    }
}

/// Extracts active screen settings aligned with the monitor list.
fn active_screens(settings: &AppSettings, monitor_count: usize) -> Vec<ScreenSettings> {
    settings.screens.iter().filter(|screen| screen.id > 0).take(monitor_count).cloned().collect()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Computes the per-user GNOME Shell extension directory.
#[must_use]
pub fn extension_directory(xdg_data_home: Option<&Path>, home: Option<&Path>) -> PathBuf {
    let base = xdg_data_home
        .filter(|path| !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from(".local/share"));
    base.join("gnome-shell").join("extensions").join(EXTENSION_UUID)
}

fn local_timestamp() -> String {
    glib::DateTime::now_local()
        .ok()
        .and_then(|date| date.format("%Y-%m-%d %H:%M:%S").ok())
        .map_or_else(now_string, |formatted| formatted.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::field_reassign_with_default)]

    use super::*;

    #[test]
    fn extension_directory_prefers_xdg() {
        let directory =
            extension_directory(Some(Path::new("/tmp/data")), Some(Path::new("/home/u")));
        assert_eq!(directory, Path::new("/tmp/data/gnome-shell/extensions/dimmer@dimmer.linux"));
    }

    #[test]
    fn extension_directory_falls_back_to_home() {
        let directory = extension_directory(None, Some(Path::new("/home/u")));
        assert_eq!(
            directory,
            Path::new("/home/u/.local/share/gnome-shell/extensions/dimmer@dimmer.linux")
        );
    }

    #[test]
    fn active_screens_skip_orphans() {
        let mut settings = AppSettings::default();
        settings.screens = vec![
            ScreenSettings { id: 1, ..ScreenSettings::default() },
            ScreenSettings { id: 2, ..ScreenSettings::default() },
            ScreenSettings { id: 0, ..ScreenSettings::default() },
        ];
        let active = active_screens(&settings, 2);
        assert_eq!(active.len(), 2);
        assert_eq!(active[1].id, 2);
    }
}
