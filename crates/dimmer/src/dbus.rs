//! D-Bus service and helper for talking to an already running instance.

use std::sync::Arc;
use std::time::Duration;

use zbus::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

use crate::shared::{DBUS_INTERFACE, DBUS_NAME, DBUS_PATH, SharedState};

/// zbus interface implementation.
struct DimmerService {
    shared: Arc<SharedState>,
}

#[zbus::interface(name = "io.github.dimmer.Linux")]
impl DimmerService {
    /// Returns a protocol/version banner.
    #[zbus(name = "Ping")]
    #[allow(clippy::unused_self)]
    fn ping(&self) -> String {
        SharedState::ping()
    }

    /// Returns the overlay state document.
    #[zbus(name = "GetState")]
    fn get_state(&self) -> String {
        self.shared.state_json()
    }

    /// Registers the GNOME extension and returns the current state.
    #[zbus(name = "RegisterExtension")]
    #[allow(clippy::needless_pass_by_value)]
    fn register_extension(&self, extension_version: String) -> String {
        self.shared.register_extension(&extension_version)
    }

    /// Clears the extension registration when the extension is disabled.
    #[zbus(name = "ExtensionDisconnected")]
    fn extension_disconnected(&self) {
        self.shared.unregister_extension();
    }

    /// Shows the settings window.
    #[zbus(name = "ShowWindow")]
    fn show_window(&self) {
        self.shared.send_ui(crate::shared::UiCommand::ShowWindow);
    }

    /// Hides the settings window.
    #[zbus(name = "HideWindow")]
    fn hide_window(&self) {
        self.shared.send_ui(crate::shared::UiCommand::HideWindow);
    }

    /// Presents the window at its default size.
    #[zbus(name = "ResetWindowPosition")]
    fn reset_window_position(&self) {
        self.shared.send_ui(crate::shared::UiCommand::ResetWindowPosition);
    }

    /// Updates the master dimmer.
    #[zbus(name = "SetMaster")]
    fn set_master(&self, on: bool, dim: f64) {
        self.shared.api_set_master(on, dim);
    }

    /// Updates one screen.
    #[zbus(name = "SetScreen")]
    fn set_screen(&self, id: i32, on: bool, dim: f64) {
        self.shared.api_set_screen(id, on, dim);
    }

    /// Saves and quits.
    #[zbus(name = "Quit")]
    fn quit(&self) {
        self.shared.send_ui(crate::shared::UiCommand::Quit);
    }
}

/// Connects to the session bus and claims the well-known name.
///
/// The connection is returned so the caller can keep it alive for the whole
/// process lifetime; dropping it releases the name and tears down the service.
///
/// # Errors
///
/// Returns [`zbus::Error::NameTaken`] when another instance is already running,
/// or another connection error.
pub async fn connect_named() -> zbus::Result<Connection> {
    let connection = Connection::session().await?;
    // Do not allow replacement: the first instance must keep the name so a
    // second launch can hand its request to the running window instead of
    // silently starting a second copy.  DO_NOT_QUEUE makes the request fail
    // immediately rather than waiting for the owner to exit.
    let reply =
        connection.request_name_with_flags(DBUS_NAME, RequestNameFlags::DO_NOT_QUEUE).await?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(connection),
        RequestNameReply::InQueue | RequestNameReply::Exists => Err(zbus::Error::NameTaken),
    }
}

/// Registers the service object on an already named connection.
///
/// # Errors
///
/// Returns an error when the object path cannot be exported.
pub async fn serve(connection: &Connection, shared: Arc<SharedState>) -> zbus::Result<()> {
    let service = DimmerService { shared: Arc::clone(&shared) };
    connection.object_server().at(DBUS_PATH, service).await?;
    shared.log().info(format!("D-Bus service listening on {DBUS_NAME}{DBUS_PATH}"));
    shared.attach_connection(connection.clone());
    Ok(())
}

/// Calls a parameterless method on an already running instance.
///
/// # Errors
///
/// Returns a zbus error when the service is absent or the call fails.
pub async fn call_remote(method: &str) -> zbus::Result<()> {
    let connection = Connection::session().await?;
    connection.call_method(Some(DBUS_NAME), DBUS_PATH, Some(DBUS_INTERFACE), method, &()).await?;
    Ok(())
}

/// Calls a method, retrying briefly while a freshly started peer exports it.
///
/// # Errors
///
/// Returns the last zbus error when every attempt fails.
pub async fn call_remote_with_retry(method: &str) -> zbus::Result<()> {
    const ATTEMPTS: u32 = 40;
    let mut attempt = 1;
    loop {
        match call_remote(method).await {
            Ok(()) => return Ok(()),
            Err(_) if attempt < ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }
}
