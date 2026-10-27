//! Options page: display toggles, startup and window behaviour.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;

use crate::shared::{SharedState, Snapshot, ToggleOption};
use crate::ui::widgets::name_group_list;

struct Rows {
    show_ids: adw::SwitchRow,
    show_boundaries: adw::SwitchRow,
    non_dimmable: adw::SwitchRow,
    auto_run: adw::SwitchRow,
    start_minimised: adw::SwitchRow,
    movable: adw::SwitchRow,
    close_hides: adw::SwitchRow,
    show_in_taskbar: adw::SwitchRow,
}

/// The Options tab.
pub struct OptionsPage {
    page: adw::PreferencesPage,
    rows: Rows,
    guard: Rc<Cell<bool>>,
    movable: Rc<Cell<bool>>,
    flatpak: bool,
}

impl OptionsPage {
    #[must_use]
    #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
    pub fn new(
        shared: Arc<SharedState>,
        toast: Rc<dyn Fn(&str)>,
        reset_window: Rc<dyn Fn()>,
        movable: Rc<Cell<bool>>,
    ) -> Self {
        let page = adw::PreferencesPage::new();
        let guard = Rc::new(Cell::new(false));

        let display = adw::PreferencesGroup::new();
        display.set_title("Display");
        name_group_list(&display, "Display");
        let show_ids =
            switch_row("Show screen identity", "Puts a large identity number on each screen.");
        let show_boundaries =
            switch_row("Show screen boundaries", "Draws a 5 px yellow border around each screen.");
        let non_dimmable = switch_row(
            "Non dimmable window",
            "Keeps this settings window bright when dimming is active.",
        );
        display.add(&show_ids);
        display.add(&show_boundaries);
        display.add(&non_dimmable);

        let startup = adw::PreferencesGroup::new();
        startup.set_title("Startup");
        name_group_list(&startup, "Startup");
        startup
            .set_description(Some("Autostart uses an XDG desktop entry and needs no root access."));
        let auto_run = switch_row("Auto run", "Start Dimmer automatically after you log in.");
        let is_flatpak = std::env::var_os("FLATPAK_ID").is_some();
        if is_flatpak {
            auto_run.set_subtitle(
                "Not available in Flatpak. Use your desktop's startup applications settings instead.",
            );
            auto_run.set_sensitive(false);
        }
        let start_minimised =
            switch_row("Start minimised", "Start hidden in the background with dimming active.");
        startup.add(&auto_run);
        startup.add(&start_minimised);

        let window = adw::PreferencesGroup::new();
        window.set_title("Window");
        name_group_list(&window, "Window");
        let movable_row =
            switch_row("Movable window", "Allow dragging the window by its header bar.");
        let reset_row = adw::ActionRow::builder()
            .title("Window position")
            .subtitle("Center the default-size window on the primary monitor and bring it forward.")
            .build();
        let reset = gtk::Button::with_label("Reset");
        reset.set_valign(gtk::Align::Center);
        reset.set_tooltip_text(Some("Reset the saved window position"));
        reset_row.add_suffix(&reset);
        let close_hides = switch_row(
            "Close button hides to tray",
            "Closing keeps dimming active; use the top-bar indicator to reopen or quit.",
        );
        let show_in_taskbar = switch_row(
            "Show in taskbar",
            "Stored for compatibility. GTK4 cannot control taskbar visibility on Wayland.",
        );
        show_in_taskbar.set_subtitle(
            "Not available on Wayland. The preference is stored for settings compatibility.",
        );
        show_in_taskbar.set_sensitive(false);
        window.add(&movable_row);
        window.add(&reset_row);
        window.add(&close_hides);
        window.add(&show_in_taskbar);

        page.add(&display);
        page.add(&startup);
        page.add(&window);

        let rows = Rows {
            show_ids,
            show_boundaries,
            non_dimmable,
            auto_run,
            start_minimised,
            movable: movable_row,
            close_hides,
            show_in_taskbar,
        };

        connect_toggle(&rows.show_ids, ToggleOption::ShowIds, &shared, &guard, &toast);
        connect_toggle(
            &rows.show_boundaries,
            ToggleOption::ShowBoundaries,
            &shared,
            &guard,
            &toast,
        );
        connect_toggle(
            &rows.non_dimmable,
            ToggleOption::NonDimmableWindow,
            &shared,
            &guard,
            &toast,
        );
        connect_toggle(
            &rows.start_minimised,
            ToggleOption::StartMinimised,
            &shared,
            &guard,
            &toast,
        );
        connect_toggle(&rows.movable, ToggleOption::MovableWindow, &shared, &guard, &toast);
        connect_toggle(&rows.close_hides, ToggleOption::CloseHidesToTray, &shared, &guard, &toast);
        connect_toggle(&rows.show_in_taskbar, ToggleOption::ShowInTaskbar, &shared, &guard, &toast);

        {
            let shared = Arc::clone(&shared);
            let guard = Rc::clone(&guard);
            let toast = Rc::clone(&toast);
            rows.auto_run.connect_active_notify(move |row| {
                if guard.get() {
                    return;
                }
                if let Err(error) = shared.set_autostart(row.is_active()) {
                    toast(&error);
                    guard.set(true);
                    row.set_active(!row.is_active());
                    guard.set(false);
                }
            });
        }
        {
            let reset_window = Rc::clone(&reset_window);
            reset.connect_clicked(move |_| reset_window());
        }

        Self { page, rows, guard, movable, flatpak: is_flatpak }
    }

    #[must_use]
    pub const fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }

    pub fn refresh(&self, snapshot: &Snapshot) {
        self.guard.set(true);
        self.rows.show_ids.set_active(snapshot.settings.show_ids);
        self.rows.show_boundaries.set_active(snapshot.settings.show_boundaries);
        self.rows.non_dimmable.set_active(snapshot.settings.non_dimmable_window);
        self.rows.auto_run.set_active(!self.flatpak && snapshot.settings.auto_run.on);
        self.rows.start_minimised.set_active(snapshot.settings.start_minimised);
        self.rows.movable.set_active(snapshot.settings.movable_window);
        self.rows.close_hides.set_active(snapshot.settings.close_hides_to_tray);
        self.rows.show_in_taskbar.set_active(snapshot.settings.show_in_taskbar);
        self.movable.set(snapshot.settings.movable_window);
        self.guard.set(false);
    }
}

fn switch_row(title: &str, subtitle: &str) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_activatable(true);
    row
}

fn connect_toggle(
    row: &adw::SwitchRow,
    option: ToggleOption,
    shared: &Arc<SharedState>,
    guard: &Rc<Cell<bool>>,
    toast: &Rc<dyn Fn(&str)>,
) {
    let shared = Arc::clone(shared);
    let guard = Rc::clone(guard);
    let toast = Rc::clone(toast);
    row.connect_active_notify(move |row| {
        if guard.get() {
            return;
        }
        if let Err(error) = shared.set_toggle(option, row.is_active()) {
            toast(&error);
            guard.set(true);
            row.set_active(!row.is_active());
            guard.set(false);
        }
    });
}
