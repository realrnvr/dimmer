//! Screens page: master card and one vertical dimmer card per monitor.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;

use crate::shared::{SharedState, Snapshot};
use crate::ui::widgets::{SaveDebouncer, color_from_hex, hex_from_color, name_group_list};

/// Widgets and state for one screen card.
struct ScreenCard {
    id: i32,
    adjustment: gtk::Adjustment,
    checkbox: gtk::CheckButton,
    /// Plain button + swatch instead of `gtk::ColorDialogButton`: the latter
    /// exposes itself as a non-focusable `group`, which hides the tint control
    /// from screen readers and makes keyboard focus disappear from AT-SPI.
    tint_swatch: gtk::DrawingArea,
    tint_color: Rc<RefCell<gtk::gdk::RGBA>>,
    tint_hex: Rc<RefCell<String>>,
    subtitle: gtk::Label,
    guard: Rc<Cell<bool>>,
}

/// Widgets and state for the master card.
struct MasterCard {
    adjustment: gtk::Adjustment,
    checkbox: gtk::CheckButton,
    controls: gtk::Grid,
    subtitle: gtk::Label,
    guard: Rc<Cell<bool>>,
}

/// The Screens tab.
pub struct ScreensPage {
    page: adw::PreferencesPage,
    flow: gtk::FlowBox,
    master: Option<MasterCard>,
    cards: Vec<ScreenCard>,
    signature: String,
    shared: Arc<SharedState>,
    debouncer: Rc<SaveDebouncer>,
}

impl ScreensPage {
    #[must_use]
    pub fn new(shared: Arc<SharedState>, debouncer: Rc<SaveDebouncer>) -> Self {
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        group.set_title("Screens");
        name_group_list(&group, "Screens");
        group.set_description(Some(
            "Dim each display independently. The tint colour is mixed into the dimming overlay.",
        ));

        let flow = gtk::FlowBox::new();
        flow.set_max_children_per_line(4);
        flow.set_min_children_per_line(1);
        flow.set_homogeneous(true);
        flow.set_row_spacing(14);
        flow.set_column_spacing(14);
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_valign(gtk::Align::Start);
        flow.set_halign(gtk::Align::Fill);
        group.add(&flow);
        page.add(&group);

        Self {
            page,
            flow,
            master: None,
            cards: Vec::new(),
            signature: String::new(),
            shared,
            debouncer,
        }
    }

    #[must_use]
    pub const fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }

    /// Applies a snapshot, rebuilding cards only when the monitor layout changed.
    pub fn refresh(&mut self, snapshot: &Snapshot) {
        let signature = signature(snapshot);
        if signature != self.signature {
            self.signature = signature;
            self.rebuild(snapshot);
        }

        if let Some(master) = &self.master {
            refresh_master(master, snapshot);
        }
        for card in &self.cards {
            refresh_card(card, snapshot);
        }
    }

    fn rebuild(&mut self, snapshot: &Snapshot) {
        while let Some(child) = self.flow.first_child() {
            self.flow.remove(&child);
        }
        self.master = None;
        self.cards.clear();

        if snapshot.monitors.is_empty() {
            let empty = gtk::Label::new(Some(
                "No monitors detected.\nConnect a display and Dimmer will pick it up automatically.",
            ));
            empty.set_justify(gtk::Justification::Center);
            empty.add_css_class("dim-label");
            empty.set_margin_top(36);
            empty.set_margin_bottom(36);
            empty.set_hexpand(true);
            self.flow.append(&empty);
            return;
        }

        let master = self.build_master();
        self.flow.append(&master.0);
        self.master = Some(master.1);
        for screen in &snapshot.active {
            let (card_widget, card) = self.build_card(screen.id);
            self.flow.append(&card_widget);
            self.cards.push(card);
        }
    }

    fn build_master(&self) -> (gtk::Box, MasterCard) {
        let guard = Rc::new(Cell::new(false));
        let card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        card.add_css_class("card");
        card.add_css_class("dimmer-card");
        card.add_css_class("dimmer-master");
        card.set_width_request(230);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let checkbox = gtk::CheckButton::with_label("Master control");
        checkbox.set_hexpand(true);
        checkbox.set_halign(gtk::Align::Start);
        checkbox.set_tooltip_text(Some(
            "Move every screen together. Available with two or more displays.",
        ));
        let icon = gtk::Image::from_icon_name("video-display-symbolic");
        icon.add_css_class("dim-label");
        header.append(&checkbox);
        header.append(&icon);

        let adjustment = gtk::Adjustment::new(30.0, 0.0, 90.0, 1.0, 10.0, 0.0);
        let controls = scale_and_spin(&adjustment, false, "Master dim percentage (0–90)");
        let subtitle = gtk::Label::new(None);
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");
        subtitle.set_text("Moves all screens together");

        card.append(&header);
        card.append(&controls);
        card.append(&subtitle);

        {
            let shared = Arc::clone(&self.shared);
            let debouncer = Rc::clone(&self.debouncer);
            let guard = Rc::clone(&guard);
            checkbox.connect_toggled(move |checkbox| {
                if guard.get() {
                    return;
                }
                shared.ui_set_master_on(checkbox.is_active());
                debouncer.schedule();
            });
        }
        {
            let shared = Arc::clone(&self.shared);
            let debouncer = Rc::clone(&self.debouncer);
            let guard = Rc::clone(&guard);
            adjustment.connect_value_changed(move |adjustment| {
                if guard.get() {
                    return;
                }
                shared.ui_set_master_dim(adjustment.value());
                debouncer.schedule();
            });
        }

        (card, MasterCard { adjustment, checkbox, controls, subtitle, guard })
    }

    fn build_card(&self, id: i32) -> (gtk::Box, ScreenCard) {
        let guard = Rc::new(Cell::new(false));
        let card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        card.add_css_class("card");
        card.add_css_class("dimmer-card");
        card.set_width_request(230);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let checkbox = gtk::CheckButton::with_label(&format!("Screen {id}"));
        checkbox.set_hexpand(true);
        checkbox.set_halign(gtk::Align::Start);
        checkbox.set_tooltip_text(Some("Enable or disable this screen's dimming"));

        let tint = self.build_tint(id, &guard);
        header.append(&checkbox);
        header.append(&tint.button);

        let adjustment = gtk::Adjustment::new(30.0, 0.0, 90.0, 1.0, 10.0, 0.0);
        let controls =
            scale_and_spin(&adjustment, true, &format!("Screen {id} dim percentage (0–90)"));
        let subtitle = gtk::Label::new(None);
        subtitle.set_xalign(0.0);
        subtitle.set_wrap(true);
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");

        card.append(&header);
        card.append(&controls);
        card.append(&subtitle);

        {
            let shared = Arc::clone(&self.shared);
            let debouncer = Rc::clone(&self.debouncer);
            let guard = Rc::clone(&guard);
            checkbox.connect_toggled(move |checkbox| {
                if guard.get() {
                    return;
                }
                shared.ui_set_screen_on(id, checkbox.is_active());
                debouncer.schedule();
            });
        }
        {
            let shared = Arc::clone(&self.shared);
            let debouncer = Rc::clone(&self.debouncer);
            let guard = Rc::clone(&guard);
            adjustment.connect_value_changed(move |adjustment| {
                if guard.get() {
                    return;
                }
                shared.ui_set_screen_dim(id, adjustment.value());
                debouncer.schedule();
            });
        }
        let tint_hex = tint.hex;

        (
            card,
            ScreenCard {
                id,
                adjustment,
                checkbox,
                tint_swatch: tint.swatch,
                tint_color: tint.color,
                tint_hex,
                subtitle,
                guard,
            },
        )
    }

    fn build_tint(&self, id: i32, guard: &Rc<Cell<bool>>) -> TintControls {
        let button = gtk::Button::new();
        button.add_css_class("dimmer-tint");
        button.set_tooltip_text(Some("Tint colour for this screen's dimming overlay"));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Screen {id} tint colour"
        ))]);

        let swatch = gtk::DrawingArea::new();
        swatch.set_size_request(18, 18);
        swatch.set_can_target(false);
        let color = Rc::new(RefCell::new(color_from_hex("000000")));
        {
            let color = Rc::clone(&color);
            swatch.set_draw_func(move |_, context, width, height| {
                let color = color.borrow();
                let center_x = f64::from(width) / 2.0;
                let center_y = f64::from(height) / 2.0;
                let radius = (f64::from(width.min(height)) / 2.0 - 1.0).max(0.0);
                context.arc(center_x, center_y, radius, 0.0, std::f64::consts::TAU);
                context.set_source_rgb(
                    f64::from(color.red()),
                    f64::from(color.green()),
                    f64::from(color.blue()),
                );
                if context.fill_preserve().is_ok() {
                    context.set_source_rgba(0.0, 0.0, 0.0, 0.25);
                    context.set_line_width(1.0);
                    let _ = context.stroke();
                }
            });
        }
        paint_tint(&swatch, &color, "000000");
        button.set_child(Some(&swatch));

        let hex = Rc::new(RefCell::new("000000".to_owned()));
        {
            let shared = Arc::clone(&self.shared);
            let debouncer = Rc::clone(&self.debouncer);
            let guard = Rc::clone(guard);
            let swatch = swatch.clone();
            let color = Rc::clone(&color);
            let hex = Rc::clone(&hex);
            button.connect_clicked(move |button| {
                let dialog = gtk::ColorDialog::new();
                dialog.set_title("Screen tint");
                dialog.set_with_alpha(false);
                dialog.set_modal(true);
                let parent = button.root().and_then(|root| root.downcast::<gtk::Window>().ok());
                let initial = color_from_hex(&hex.borrow());
                let shared = Arc::clone(&shared);
                let debouncer = Rc::clone(&debouncer);
                let guard = Rc::clone(&guard);
                let swatch = swatch.clone();
                let color = Rc::clone(&color);
                let hex = Rc::clone(&hex);
                dialog.choose_rgba(
                    parent.as_ref(),
                    Some(&initial),
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        let Ok(rgba) = result else {
                            return;
                        };
                        let value = hex_from_color(&rgba);
                        paint_tint(&swatch, &color, &value);
                        hex.borrow_mut().clone_from(&value);
                        if guard.get() {
                            return;
                        }
                        shared.ui_set_screen_tint(id, &value);
                        debouncer.schedule();
                    },
                );
            });
        }

        TintControls { button, swatch, color, hex }
    }
}

/// Widgets and state for a screen's tint picker.
struct TintControls {
    button: gtk::Button,
    swatch: gtk::DrawingArea,
    color: Rc<RefCell<gtk::gdk::RGBA>>,
    hex: Rc<RefCell<String>>,
}

fn signature(snapshot: &Snapshot) -> String {
    snapshot
        .monitors
        .iter()
        .map(|monitor| {
            format!(
                "{}:{}x{}+{}+{}@{}",
                monitor.connector,
                monitor.geometry.width,
                monitor.geometry.height,
                monitor.geometry.x,
                monitor.geometry.y,
                monitor.scale
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn refresh_master(master: &MasterCard, snapshot: &Snapshot) {
    let available = snapshot.active.len() > 1;
    let on = snapshot.settings.master.on && available;
    master.guard.set(true);
    master.checkbox.set_active(on);
    master.checkbox.set_sensitive(available);
    master.adjustment.set_value(snapshot.settings.master.dim);
    master.controls.set_sensitive(on);
    master.controls.set_opacity(if on { 1.0 } else { 0.55 });
    master.subtitle.set_text(if available {
        if on { "Controlling all screens" } else { "Moves all screens together" }
    } else {
        "Connect a second display to use master control"
    });
    master.guard.set(false);
}

fn refresh_card(card: &ScreenCard, snapshot: &Snapshot) {
    let Some(screen) = snapshot.active.iter().find(|screen| screen.id == card.id) else {
        return;
    };
    let Some(monitor) = snapshot.monitors.get(usize::try_from(card.id - 1).unwrap_or(0)) else {
        return;
    };
    card.guard.set(true);
    card.checkbox.set_active(screen.on);
    card.adjustment.set_value(screen.dim);
    if *card.tint_hex.borrow() != screen.tint {
        paint_tint(&card.tint_swatch, &card.tint_color, &screen.tint);
        card.tint_hex.borrow_mut().clone_from(&screen.tint);
    }
    card.subtitle.set_text(&format!(
        "{} · {}×{}{}",
        monitor.connector,
        monitor.geometry.width,
        monitor.geometry.height,
        if monitor.primary { " · primary" } else { "" }
    ));
    card.guard.set(false);
}

fn scale_and_spin(adjustment: &gtk::Adjustment, wheel: bool, label: &str) -> gtk::Grid {
    // The slider and the numeric controls are siblings in one grid rather than
    // a box inside a box: libadwaita's chrome already nests the cards ~20
    // levels deep, and AT-SPI clients that cap tree depth (WayDriver stops at
    // 20) would otherwise lose the controls entirely. Everything here stays a
    // direct child of the grid, at the same depth as the slider.
    let row = gtk::Grid::new();
    row.set_column_spacing(6);
    row.set_row_spacing(4);
    row.set_row_homogeneous(true);
    row.set_halign(gtk::Align::Center);
    row.set_valign(gtk::Align::Center);

    let scale = gtk::Scale::new(gtk::Orientation::Vertical, Some(adjustment));
    scale.set_inverted(false);
    scale.set_draw_value(false);
    scale.set_size_request(46, 190);
    scale.set_valign(gtk::Align::Center);
    scale.update_property(&[gtk::accessible::Property::Label(label)]);
    scale.add_mark(0.0, gtk::PositionType::Bottom, Some("0"));
    scale.add_mark(30.0, gtk::PositionType::Bottom, None);
    scale.add_mark(60.0, gtk::PositionType::Bottom, None);
    scale.add_mark(90.0, gtk::PositionType::Top, Some("90"));
    if wheel {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let adjustment = adjustment.clone();
        scroll.connect_scroll(move |_, _dx, dy| {
            let delta = if dy < 0.0 { 1.0 } else { -1.0 };
            adjustment.set_value((adjustment.value() + delta).clamp(0.0, 90.0));
            gtk::glib::Propagation::Stop
        });
        scale.add_controller(scroll);
    }

    // A `GtkSpinButton` is deliberately not used here: on GTK 4.22 its
    // accessibility is delegated to an internal entry that never appears in
    // the AT-SPI tree, so keyboard focus vanishes for screen readers. Buttons
    // plus a plain `GtkEntry` keep the same functionality and stay exposed.
    let up = step_button("value-increase-symbolic", &format!("Increase {label}"));
    let down = step_button("value-decrease-symbolic", &format!("Decrease {label}"));
    let entry = gtk::Entry::new();
    entry.set_width_request(82);
    entry.set_max_width_chars(3);
    entry.set_valign(gtk::Align::Center);
    entry.set_halign(gtk::Align::Center);
    gtk::prelude::EditableExt::set_alignment(&entry, 0.5);
    entry.set_input_purpose(gtk::InputPurpose::Digits);
    entry.set_tooltip_text(Some("Dim percentage (0–90)"));
    entry.update_property(&[gtk::accessible::Property::Label(label)]);
    entry.set_text(&format!("{:.0}", adjustment.value()));

    let commit: Rc<dyn Fn()> = {
        let adjustment = adjustment.clone();
        let entry = entry.clone();
        Rc::new(move || {
            let text = entry.text();
            let Ok(value) = text.trim().parse::<f64>() else {
                entry.set_text(&format!("{:.0}", adjustment.value()));
                return;
            };
            if !value.is_finite() {
                entry.set_text(&format!("{:.0}", adjustment.value()));
                return;
            }
            let clamped = value.clamp(adjustment.lower(), adjustment.upper());
            adjustment.set_value(clamped);
            entry.set_text(&format!("{clamped:.0}"));
        })
    };
    entry.connect_activate({
        let commit = Rc::clone(&commit);
        move |_| commit()
    });
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave({
        let commit = Rc::clone(&commit);
        move |_| commit()
    });
    entry.add_controller(focus);

    for (button, step) in [(&up, 1.0_f64), (&down, -1.0_f64)] {
        let adjustment = adjustment.clone();
        button.connect_clicked(move |_| {
            let value = step
                .mul_add(adjustment.step_increment(), adjustment.value())
                .clamp(adjustment.lower(), adjustment.upper());
            adjustment.set_value(value);
        });
    }
    {
        let entry = entry.clone();
        adjustment.connect_value_changed(move |adjustment| {
            if !entry.has_focus() {
                entry.set_text(&format!("{:.0}", adjustment.value()));
            }
        });
    }

    let percent = gtk::Label::new(Some("percent"));
    percent.add_css_class("dim-label");
    percent.add_css_class("caption");
    percent.set_halign(gtk::Align::Center);

    row.attach(&scale, 0, 0, 1, 4);
    row.attach(&up, 1, 0, 1, 1);
    row.attach(&entry, 1, 1, 1, 1);
    row.attach(&down, 1, 2, 1, 1);
    row.attach(&percent, 1, 3, 1, 1);
    row
}

fn step_button(icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.set_size_request(34, 34);
    button.set_halign(gtk::Align::Center);
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some(label));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

fn paint_tint(swatch: &gtk::DrawingArea, color: &RefCell<gtk::gdk::RGBA>, hex: &str) {
    *color.borrow_mut() = color_from_hex(hex);
    swatch.queue_draw();
}
