//! Debug page: live status, copyable diagnostics and log.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;

use crate::shared::SharedState;
use crate::ui::widgets::name_group_list;

struct StatusRows {
    application: adw::ActionRow,
    extension: adw::ActionRow,
    session: adw::ActionRow,
    monitors: adw::ActionRow,
    settings: adw::ActionRow,
    autostart: adw::ActionRow,
}

/// The Debug tab.
pub struct DebugPage {
    page: adw::PreferencesPage,
    rows: StatusRows,
    view: gtk::TextView,
    text: Rc<RefCell<String>>,
    shared: Arc<SharedState>,
    toast: Rc<dyn Fn(&str)>,
}

impl DebugPage {
    #[must_use]
    pub fn new(shared: Arc<SharedState>, toast: Rc<dyn Fn(&str)>) -> Rc<Self> {
        let page = adw::PreferencesPage::new();
        let rows = StatusRows {
            application: status_row("Application"),
            extension: status_row("GNOME extension"),
            session: status_row("Session"),
            monitors: status_row("Monitors"),
            settings: status_row("Settings file"),
            autostart: status_row("Autostart"),
        };
        let status = adw::PreferencesGroup::new();
        status.set_title("Status");
        name_group_list(&status, "Status");
        status.add(&rows.application);
        status.add(&rows.extension);
        status.add(&rows.session);
        status.add(&rows.monitors);
        status.add(&rows.settings);
        status.add(&rows.autostart);
        page.add(&status);

        let diagnostics = adw::PreferencesGroup::new();
        diagnostics.set_title("Diagnostics");
        name_group_list(&diagnostics, "Diagnostics");
        diagnostics.set_description(Some(
            "Copy this report when asking for help. Settings paths are included; no telemetry is sent.",
        ));
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let refresh = gtk::Button::with_label("Refresh");
        refresh.add_css_class("flat");
        let copy = gtk::Button::with_label("Copy");
        copy.add_css_class("flat");
        buttons.append(&refresh);
        buttons.append(&copy);
        diagnostics.set_header_suffix(Some(&buttons));

        let view = gtk::TextView::new();
        view.update_property(&[gtk::accessible::Property::Label("Diagnostics report")]);
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_monospace(true);
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_top_margin(12);
        view.set_bottom_margin(12);
        view.set_left_margin(12);
        view.set_right_margin(12);
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(360);
        scrolled.set_child(Some(&view));
        let frame = gtk::Frame::new(None);
        frame.add_css_class("card");
        frame.set_child(Some(&scrolled));
        diagnostics.add(&frame);
        page.add(&diagnostics);

        let folders = adw::PreferencesGroup::new();
        folders.set_title("Folders");
        name_group_list(&folders, "Folders");
        let settings_row = folder_row("Open settings folder");
        let extension_row = folder_row("Open extension folder");
        folders.add(&settings_row);
        folders.add(&extension_row);
        page.add(&folders);

        let page = Rc::new(Self {
            page,
            rows,
            view,
            text: Rc::new(RefCell::new(String::new())),
            shared,
            toast,
        });
        page.connect_buttons(&refresh, &copy, &settings_row, &extension_row);
        page.refresh();
        page
    }

    #[must_use]
    pub const fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Rebuilds the report and status rows from current state.
    pub fn refresh(&self) {
        let snapshot = self.shared.snapshot();
        self.rows.application.set_subtitle(&format!(
            "Dimmer {} · protocol {}",
            crate::APP_VERSION,
            dimmer_core::PROTOCOL_VERSION
        ));
        self.rows.extension.set_subtitle(&extension_status(&snapshot));
        self.rows.session.set_subtitle(&format!(
            "{} · {} · GNOME Shell {}",
            self.shared.session().session_type,
            self.shared.session().desktop,
            self.shared.session().shell_version
        ));
        self.rows.monitors.set_subtitle(&if snapshot.monitors.is_empty() {
            "No monitors reported".to_owned()
        } else {
            format!(
                "{} connected: {}",
                snapshot.monitors.len(),
                snapshot
                    .monitors
                    .iter()
                    .map(|monitor| monitor.connector.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        });
        self.rows.settings.set_subtitle(&snapshot.settings_path.display().to_string());
        let autostart = self.shared.autostart_path().map_or_else(
            || "unavailable (HOME is not set)".to_owned(),
            |path| {
                format!(
                    "{} · {}",
                    path.display(),
                    if path.is_file() { "enabled" } else { "disabled" }
                )
            },
        );
        self.rows.autostart.set_subtitle(&autostart);

        let report = self.shared.diagnostics();
        if let Ok(mut text) = self.text.try_borrow_mut() {
            text.clone_from(&report);
        }
        self.view.buffer().set_text(&report);
    }

    fn connect_buttons(
        self: &Rc<Self>,
        refresh: &gtk::Button,
        copy: &gtk::Button,
        open_settings: &adw::ActionRow,
        open_extension: &adw::ActionRow,
    ) {
        {
            let weak = Rc::downgrade(self);
            refresh.connect_clicked(move |_| {
                if let Some(page) = weak.upgrade() {
                    page.refresh();
                }
            });
        }
        {
            let text = Rc::clone(&self.text);
            let toast = Rc::clone(&self.toast);
            copy.connect_clicked(move |button| {
                let report = text.try_borrow().map_or_else(|_| String::new(), |text| text.clone());
                button.clipboard().set_text(&report);
                toast("Diagnostics copied to the clipboard");
            });
        }
        {
            let shared = Arc::clone(&self.shared);
            let toast = Rc::clone(&self.toast);
            open_settings.connect_activated(move |_| {
                if let Some(parent) = shared.settings_path().parent() {
                    open_folder(parent, &toast);
                }
            });
        }
        {
            let shared = Arc::clone(&self.shared);
            let toast = Rc::clone(&self.toast);
            open_extension.connect_activated(move |_| {
                let directory = shared.extension_dir();
                let target = if directory.is_dir() {
                    directory.to_path_buf()
                } else {
                    directory.parent().unwrap_or(directory).to_path_buf()
                };
                open_folder(&target, &toast);
            });
        }
    }
}

fn extension_status(snapshot: &crate::shared::Snapshot) -> String {
    if snapshot.extension_connected {
        snapshot.extension_version.as_ref().map_or_else(
            || "connected".to_owned(),
            |version| format!("connected · extension {version}"),
        )
    } else if snapshot.extension_installed {
        "installed, not connected (log out and back in)".to_owned()
    } else {
        "not installed".to_owned()
    }
}

fn status_row(title: &str) -> adw::ActionRow {
    adw::ActionRow::builder().title(title).subtitle("—").build()
}

fn folder_row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(&gtk::Image::from_icon_name("folder-open-symbolic"));
    row.set_activatable(true);
    row
}

fn open_folder(path: &std::path::Path, toast: &Rc<dyn Fn(&str)>) {
    let file = gtk::gio::File::for_path(path);
    if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
        file.uri().as_str(),
        None::<&gtk::gio::AppLaunchContext>,
    ) {
        toast(&format!("Could not open folder: {error}"));
    }
}
