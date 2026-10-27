//! Small GTK helpers shared by the pages.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::shared::SharedState;

/// Installs the application stylesheet for the default display.
pub fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../theme.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// A flat header-bar icon button.
#[must_use]
pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button.add_css_class("circular");
    button
}

/// Gives a preferences group's internal list box an accessible name.
///
/// libadwaita keeps that list box private, but it is the focusable node a
/// screen reader lands on, so the group heading alone is not enough. The walk
/// only looks at widget children, and the first list box below a group is
/// always the one that holds its rows.
pub fn name_group_list(group: &adw::PreferencesGroup, name: &str) {
    if let Some(list) = find_list_box(group.upcast_ref::<gtk::Widget>()) {
        list.update_property(&[gtk::accessible::Property::Label(name)]);
    }
}

fn find_list_box(widget: &gtk::Widget) -> Option<gtk::ListBox> {
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Ok(list) = current.clone().downcast::<gtk::ListBox>() {
            return Some(list);
        }
        if let Some(list) = find_list_box(&current) {
            return Some(list);
        }
        child = current.next_sibling();
    }
    None
}

/// Converts `RRGGBB` into a GDK colour.
#[must_use]
pub fn color_from_hex(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(format!("#{hex}")).unwrap_or_else(|_| gdk::RGBA::new(0.0, 0.0, 0.0, 1.0))
}

/// Converts a GDK colour to canonical `RRGGBB`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn hex_from_color(color: &gdk::RGBA) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "{:02X}{:02X}{:02X}",
        channel(color.red()),
        channel(color.green()),
        channel(color.blue())
    )
}

/// Shows a simple modal message.
pub fn show_message(parent: &impl IsA<gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::new(Some(parent), Some(heading), Some(body));
    dialog.add_response("close", "Close");
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    dialog.present();
}

/// Opens an external URI, showing a toast on failure.
pub fn open_uri(parent: &impl IsA<gtk::Window>, uri: &str, toast: &Rc<dyn Fn(&str)>) {
    let launcher = gtk::UriLauncher::new(uri);
    let toast = Rc::clone(toast);
    launcher.launch(Some(parent), None::<&gtk::gio::Cancellable>, move |result| {
        if let Err(error) = result {
            toast(&format!("Could not open link: {error}"));
        }
    });
}

/// Debounced settings saver used by every control.
pub struct SaveDebouncer {
    shared: Arc<SharedState>,
    generation: Cell<u64>,
    scheduled: Cell<bool>,
}

impl SaveDebouncer {
    #[must_use]
    pub fn new(shared: Arc<SharedState>) -> Rc<Self> {
        Rc::new(Self { shared, generation: Cell::new(0), scheduled: Cell::new(false) })
    }

    /// Requests a save after 400 ms of inactivity.
    pub fn schedule(self: &Rc<Self>) {
        self.generation.set(self.generation.get().wrapping_add(1));
        if self.scheduled.replace(true) {
            return;
        }
        let generation = self.generation.get();
        let this = Rc::clone(self);
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            this.scheduled.set(false);
            if this.generation.get() != generation {
                this.schedule();
                return;
            }
            if let Err(error) = this.shared.save() {
                this.shared.log().warn(format!("could not save settings: {error}"));
            }
        });
    }

    /// Saves immediately (used before quitting).
    pub fn save_now(&self) {
        if let Err(error) = self.shared.save() {
            self.shared.log().warn(format!("could not save settings: {error}"));
        }
    }
}
