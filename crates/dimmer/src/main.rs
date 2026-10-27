//! Dimmer for Linux — application entry point.
//!
//! The GTK/libadwaita process owns settings, lifecycle and the D-Bus API.  A
//! small GNOME Shell extension is the only component allowed to draw the
//! click-through dimming overlays on Wayland.

mod cli;
mod dbus;
mod extension;
mod log;
mod shared;
mod ui;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::gio;
use gtk::glib;

use dimmer_core::diagnostics;
use dimmer_core::settings::{SettingsStore, resolve_settings_path};
use shared::{SessionInfo, SharedState, UiCommand};

/// Human-readable version, re-exported for the UI modules.
pub const APP_VERSION: &str = dimmer_core::APP_VERSION;

fn main() -> glib::ExitCode {
    let options = match cli::Options::parse(std::env::args_os().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("dimmer: {error}");
            eprintln!();
            eprint!("{}", cli::Options::help_text());
            return glib::ExitCode::FAILURE;
        }
    };

    if options.help {
        print!("{}", cli::Options::help_text());
        return glib::ExitCode::SUCCESS;
    }
    if options.version {
        println!("Dimmer {}", dimmer_core::APP_VERSION);
        return glib::ExitCode::SUCCESS;
    }
    if options.install_extension {
        if std::env::var_os("FLATPAK_ID").is_some() {
            eprintln!("dimmer: a Flatpak cannot install the GNOME extension into the host.");
            eprintln!(
                "Run scripts/install-flatpak.sh from the Dimmer source checkout, or install the extension ZIP from the GitHub release on the host."
            );
            return glib::ExitCode::FAILURE;
        }
        return match extension::install() {
            Ok(directory) => {
                println!("{}", extension::instructions(&directory));
                glib::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("dimmer: {error}");
                glib::ExitCode::FAILURE
            }
        };
    }

    let runtime =
        match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("dimmer: could not start the D-Bus runtime: {error}");
                return glib::ExitCode::FAILURE;
            }
        };

    if options.quit {
        return match runtime.block_on(dbus::call_remote_with_retry("Quit")) {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("dimmer: no running instance answered Quit: {error}");
                glib::ExitCode::FAILURE
            }
        };
    }

    // Claim the bus name before touching settings.  A second launch must not
    // rewrite the settings file owned by the running instance.
    let connection = match runtime.block_on(dbus::connect_named()) {
        Ok(connection) => Some(connection),
        Err(zbus::Error::NameTaken) => {
            if options.minimised && !options.show {
                // Autostart/`--minimised` must not raise a window the user
                // already has hidden.
                return glib::ExitCode::SUCCESS;
            }
            return match runtime.block_on(dbus::call_remote_with_retry("ShowWindow")) {
                Ok(()) => glib::ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("dimmer: another instance is running but did not respond: {error}");
                    glib::ExitCode::FAILURE
                }
            };
        }
        Err(error) => {
            eprintln!(
                "dimmer: D-Bus is unavailable ({error}); single-instance and the GNOME extension will not work"
            );
            None
        }
    };

    let session = collect_session();
    let store = load_store(options.config.as_deref());
    let loaded = store.load();
    let shared = SharedState::new(store, loaded, runtime.handle().clone(), session, options.debug);
    shared.log().info(format!(
        "Dimmer {} starting ({})",
        dimmer_core::APP_VERSION,
        shared.settings_path().display()
    ));

    if let Some(warning) = shared.settings_warning() {
        shared.log().warn(warning);
    }
    if let Some(connection) = connection.as_ref() {
        if let Err(error) = runtime.block_on(dbus::serve(connection, Arc::clone(&shared))) {
            shared.record_bus_error(&format!("could not export the D-Bus service: {error}"));
        }
    }

    let exit_code = run_gtk(&shared, &options);

    shared.log().info("Dimmer exiting");
    if let Err(error) = shared.save() {
        eprintln!("dimmer: could not save settings on exit: {error}");
    }
    // Keep the bus connection (and therefore the name) owned until the GTK loop
    // has finished; release it explicitly before tearing down the runtime.
    drop(connection);
    runtime.shutdown_timeout(Duration::from_millis(750));
    exit_code
}

fn run_gtk(shared: &Arc<SharedState>, options: &cli::Options) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(dimmer_core::APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_startup(|_| {
        if let Err(error) = adw::init() {
            eprintln!("dimmer: could not initialise libadwaita: {error}");
        }
    });

    let options = options.clone();
    let ui: Rc<RefCell<Option<Rc<ui::window::Ui>>>> = Rc::new(RefCell::new(None));
    {
        let shared = Arc::clone(shared);
        let ui = Rc::clone(&ui);
        app.connect_activate(move |app| {
            if let Some(existing) = ui.borrow().as_ref() {
                existing.present(None);
                return;
            }
            let built = ui::window::Ui::build(app, Arc::clone(&shared), &options);
            *ui.borrow_mut() = Some(built);
            shared.send_ui(UiCommand::Refresh);
        });
    }

    {
        let shared = Arc::clone(shared);
        app.connect_shutdown(move |_| {
            shared.set_window_visible(false);
        });
    }

    app.run_with_args(&[dimmer_core::APP_NAME])
}

fn load_store(cli_path: Option<&Path>) -> SettingsStore {
    let exe_dir =
        std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf));
    let xdg_config = env_path("XDG_CONFIG_HOME");
    let home = env_path("HOME");
    let path =
        resolve_settings_path(cli_path, exe_dir.as_deref(), xdg_config.as_deref(), home.as_deref());
    SettingsStore::new(path)
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn collect_session() -> SessionInfo {
    SessionInfo {
        os: diagnostics::os_pretty_name(),
        session_type: env_string("XDG_SESSION_TYPE").unwrap_or_else(|| "unknown".to_owned()),
        desktop: env_string("XDG_CURRENT_DESKTOP")
            .or_else(|| env_string("XDG_SESSION_DESKTOP"))
            .unwrap_or_else(|| "unknown".to_owned()),
        shell_version: gnome_shell_version(),
    }
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn gnome_shell_version() -> String {
    let output = std::process::Command::new("gnome-shell").arg("--version").output();
    match output {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let version = text.trim().trim_start_matches("GNOME Shell").trim();
            if version.is_empty() { "unknown".to_owned() } else { version.to_owned() }
        }
        _ => "unknown".to_owned(),
    }
}
