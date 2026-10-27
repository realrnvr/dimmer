//! Main window, header bar, page stack and lifecycle wiring.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::cli::Options;
use crate::shared::{SharedState, Snapshot, UiCommand};
use crate::ui::about::AboutPage;
use crate::ui::debug::DebugPage;
use crate::ui::options::OptionsPage;
use crate::ui::screens::ScreensPage;
use crate::ui::widgets::{SaveDebouncer, icon_button, install_css, open_uri, show_message};

const DEFAULT_WIDTH: i32 = 1040;
const DEFAULT_HEIGHT: i32 = 700;
const DONATE_URL: &str = "https://www.paypal.com/donate/?business=N9EWT4F7JBWFE&item_name=Thank+you+for+your+support&item_number=Dimmer+(App)&currency_code=USD";

/// The complete settings window.
pub struct Ui {
    app: adw::Application,
    window: adw::ApplicationWindow,
    shared: Arc<SharedState>,
    stack: adw::ViewStack,
    banner: adw::Banner,
    screens: RefCell<ScreensPage>,
    options: OptionsPage,
    debug: Rc<DebugPage>,
    debouncer: Rc<SaveDebouncer>,
    quitting: Rc<Cell<bool>>,
    movable: Rc<Cell<bool>>,
    pending_window_reset: Rc<Cell<Option<u64>>>,
    toast: Rc<dyn Fn(&str)>,
}

impl Ui {
    /// Builds the whole window and attaches lifecycle handlers.
    #[must_use]
    #[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
    pub fn build(app: &adw::Application, shared: Arc<SharedState>, options: &Options) -> Rc<Self> {
        install_css();
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Dimmer")
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        window.set_decorated(false);
        window.add_css_class("dimmer-window");
        window.set_size_request(760, 500);

        let toast_overlay = adw::ToastOverlay::new();
        let toast: Rc<dyn Fn(&str)> = {
            let overlay = toast_overlay.clone();
            Rc::new(move |message: &str| {
                overlay.add_toast(adw::Toast::new(message));
            })
        };

        let debouncer = SaveDebouncer::new(Arc::clone(&shared));
        let movable = Rc::new(Cell::new(true));
        let quitting = Rc::new(Cell::new(false));

        let shared_for_reset = Arc::clone(&shared);
        let reset_window: Rc<dyn Fn()> = Rc::new(move || {
            shared_for_reset.send_ui(UiCommand::ResetWindowPosition);
        });

        let screens = ScreensPage::new(Arc::clone(&shared), Rc::clone(&debouncer));
        let options_page = OptionsPage::new(
            Arc::clone(&shared),
            Rc::clone(&toast),
            reset_window,
            Rc::clone(&movable),
        );
        let about = AboutPage::new();
        let debug = DebugPage::new(Arc::clone(&shared), Rc::clone(&toast));

        let stack = adw::ViewStack::new();
        stack.set_vexpand(true);
        stack.add_titled_with_icon(
            screens.widget(),
            Some("screens"),
            "Screens",
            "video-display-symbolic",
        );
        stack.add_titled_with_icon(
            options_page.widget(),
            Some("options"),
            "Options",
            "emblem-system-symbolic",
        );
        stack.add_titled_with_icon(about.widget(), Some("about"), "About", "help-about-symbolic");
        stack.add_titled_with_icon(
            debug.widget(),
            Some("debug"),
            "Debug",
            "utilities-terminal-symbolic",
        );

        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        header.add_css_class("dimmer-header");
        let title = adw::WindowTitle::new("Dimmer", "");
        // Drag the window only from the title cluster.  Attaching the drag
        // gesture to the whole header made `adw::ViewSwitcher` tabs and the
        // end-cap buttons unreachable: the parent gesture won the press
        // sequence before the child's click gesture could activate.  Keep a
        // dedicated, non-interactive drag area so the buttons stay clickable.
        let drag_area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        drag_area.append(&title);
        header.pack_start(&drag_area);
        let switcher = adw::ViewSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
        header.set_title_widget(Some(&switcher));

        let donate = icon_button("emblem-favorite-symbolic", "Donate with PayPal");
        let minimise = icon_button("window-minimize-symbolic", "Minimise");
        let close = icon_button("window-close-symbolic", "Close");
        header.pack_end(&close);
        header.pack_end(&minimise);
        header.pack_end(&donate);

        {
            let weak = window.downgrade();
            let shared = Arc::clone(&shared);
            let toast = Rc::clone(&toast);
            donate.connect_clicked(move |_| {
                if let Some(window) = weak.upgrade() {
                    let _ = &shared;
                    open_uri(&window, DONATE_URL, &toast);
                }
            });
        }
        {
            let weak = window.downgrade();
            let shared = Arc::clone(&shared);
            minimise.connect_clicked(move |_| {
                if let Some(window) = weak.upgrade() {
                    if shared.snapshot().settings.show_in_taskbar {
                        window.minimize();
                    } else {
                        window.set_visible(false);
                    }
                }
            });
        }
        {
            let weak = window.downgrade();
            close.connect_clicked(move |_| {
                if let Some(window) = weak.upgrade() {
                    window.close();
                }
            });
        }

        install_drag_handle(&drag_area, &window, Rc::clone(&movable));

        let banner = adw::Banner::new("");
        banner.set_revealed(false);
        banner.set_button_label(Some("Details"));
        {
            let shared = Arc::clone(&shared);
            let weak = window.downgrade();
            banner.connect_button_clicked(move |_| {
                if let Some(window) = weak.upgrade() {
                    extension_help(&shared, &window);
                }
            });
        }

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&banner);
        toolbar.set_content(Some(&stack));
        toast_overlay.set_child(Some(&toolbar));
        window.set_content(Some(&toast_overlay));

        let ui = Rc::new(Self {
            app: app.clone(),
            window: window.clone(),
            shared: Arc::clone(&shared),
            stack,
            banner,
            screens: RefCell::new(screens),
            options: options_page,
            debug,
            debouncer: Rc::clone(&debouncer),
            quitting: Rc::clone(&quitting),
            movable,
            pending_window_reset: Rc::new(Cell::new(None)),
            toast,
        });

        {
            let shared = Arc::clone(&shared);
            window.connect_visible_notify(move |window| {
                shared.set_window_visible(window.is_visible());
            });
        }
        {
            let shared = Arc::clone(&shared);
            let debouncer = Rc::clone(&debouncer);
            let quitting = Rc::clone(&quitting);
            let app = app.clone();
            window.connect_close_request(move |window| {
                debouncer.save_now();
                if shared.snapshot().settings.close_hides_to_tray && !quitting.get() {
                    window.set_visible(false);
                } else {
                    quitting.set(true);
                    app.quit();
                }
                glib::Propagation::Stop
            });
        }
        {
            let debouncer = Rc::clone(&debouncer);
            app.connect_shutdown(move |_| debouncer.save_now());
        }
        {
            let weak = Rc::downgrade(&ui);
            ui.stack.connect_visible_child_name_notify(move |stack| {
                if stack.visible_child_name().as_deref() == Some("debug") {
                    if let Some(ui) = weak.upgrade() {
                        ui.debug.refresh();
                    }
                }
            });
        }
        ui.install_monitor_watch();
        ui.install_command_pump();
        {
            let weak = Rc::downgrade(&ui);
            let shared = Arc::clone(&shared);
            let banner = ui.banner.clone();
            glib::timeout_add_local_once(Duration::from_millis(1500), move || {
                if let Some(ui) = weak.upgrade() {
                    update_banner(&shared, &banner, &ui.window);
                }
            });
        }

        ui.initialise_monitors();
        ui.refresh();

        let start_hidden = (options.minimised || shared.snapshot().settings.start_minimised)
            && !options.show
            && options.page.is_none();
        if start_hidden {
            window.set_visible(false);
        } else {
            if let Some(page) = &options.page {
                if matches!(page.as_str(), "screens" | "options" | "about" | "debug") {
                    ui.stack.set_visible_child_name(page);
                }
            }
            window.present();
        }
        ui
    }

    /// Presents the window, optionally switching page.
    pub fn present(&self, page: Option<&str>) {
        if let Some(page) = page {
            if matches!(page, "screens" | "options" | "about" | "debug") {
                self.stack.set_visible_child_name(page);
            }
        }
        self.window.present();
        self.refresh();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Brings the window back and resets the saved position hint.
    pub fn reset_position(&self) {
        self.window.present();
        if !self.shared.snapshot().extension_connected {
            (self.toast)("Window reset failed: GNOME Shell extension is not connected");
            return;
        }

        let serial = self.shared.request_window_reset();
        let _ = self.shared.save();
        self.pending_window_reset.set(Some(serial));

        let pending = Rc::clone(&self.pending_window_reset);
        let toast = Rc::clone(&self.toast);
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            if pending.get() == Some(serial) {
                pending.set(None);
                toast("Window reset failed: GNOME Shell did not confirm the new position");
            }
        });
    }

    fn complete_window_reset(&self, serial: u64, success: bool) {
        if let Some(message) = take_window_reset_result(&self.pending_window_reset, serial, success)
        {
            (self.toast)(message);
        }
    }

    /// Refreshes all pages from shared state.
    pub fn refresh(&self) {
        let snapshot = self.shared.snapshot();
        if let Ok(mut screens) = self.screens.try_borrow_mut() {
            screens.refresh(&snapshot);
        }
        self.options.refresh(&snapshot);
        self.movable.set(snapshot.settings.movable_window);
        if self.stack.visible_child_name().as_deref() == Some("debug") {
            self.debug.refresh();
        }
        update_banner(&self.shared, &self.banner, &self.window);
    }

    fn quit(&self) {
        self.quitting.set(true);
        self.debouncer.save_now();
        self.app.quit();
    }

    fn install_monitor_watch(self: &Rc<Self>) {
        let Some(display) = gdk::Display::default() else {
            self.shared.log().warn("no GDK display; monitor list unavailable");
            return;
        };
        let weak = Rc::downgrade(self);
        let shared = Arc::clone(&self.shared);
        display.monitors().connect_items_changed(move |_, _, _, _| {
            if let Some(display) = gdk::Display::default() {
                shared.set_monitors(collect_monitors(&display));
                if let Some(ui) = weak.upgrade() {
                    ui.refresh();
                }
            }
        });
    }

    fn initialise_monitors(&self) {
        if let Some(display) = gdk::Display::default() {
            self.shared.set_monitors(collect_monitors(&display));
        }
    }

    fn install_command_pump(self: &Rc<Self>) {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<UiCommand>();
        self.shared.attach_ui(&sender);
        let weak = Rc::downgrade(self);
        // Wake the GTK main loop only when a command arrives; polling every
        // 25 ms kept the process busy even while it was idle in the tray.
        glib::MainContext::default().spawn_local(async move {
            while let Some(command) = receiver.recv().await {
                let Some(ui) = weak.upgrade() else {
                    break;
                };
                match command {
                    UiCommand::ShowWindow => ui.present(None),
                    UiCommand::HideWindow => ui.hide(),
                    UiCommand::ResetWindowPosition => ui.reset_position(),
                    UiCommand::WindowResetCompleted { serial, success } => {
                        ui.complete_window_reset(serial, success);
                    }
                    UiCommand::Refresh => ui.refresh(),
                    UiCommand::Quit => {
                        ui.quit();
                        break;
                    }
                }
            }
        });
    }
}

fn take_window_reset_result(
    pending: &Cell<Option<u64>>,
    serial: u64,
    success: bool,
) -> Option<&'static str> {
    if pending.get() != Some(serial) {
        return None;
    }
    pending.set(None);
    Some(if success { "Window reset succeeded" } else { "Window reset failed" })
}

#[allow(clippy::cast_possible_truncation)]
fn install_drag_handle(
    handle: &impl IsA<gtk::Widget>,
    window: &adw::ApplicationWindow,
    movable: Rc<Cell<bool>>,
) {
    let gesture = gtk::GestureDrag::new();
    gesture.set_button(1);
    let weak = window.downgrade();
    let handle = handle.clone().upcast::<gtk::Widget>();
    handle.set_cursor_from_name(Some("grab"));
    let handle_for_gesture = handle.clone();
    gesture.connect_drag_begin(move |gesture, start_x, start_y| {
        if !movable.get() {
            return;
        }
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(surface) = window.native().and_then(|native| native.surface()) else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };
        let display: gdk::Display = gtk::prelude::WidgetExt::display(&window);
        let Some(device) = display.default_seat().and_then(|seat| seat.pointer()) else {
            return;
        };
        let origin = gtk::graphene::Point::new(start_x as f32, start_y as f32);
        let (x, y) = handle_for_gesture
            .compute_point(&window, &origin)
            .map_or((start_x, start_y), |point| (f64::from(point.x()), f64::from(point.y())));
        toplevel.begin_move(&device, 1, x, y, 0);
        let _ = gesture;
    });
    handle.add_controller(gesture);
}

fn collect_monitors(display: &gdk::Display) -> Vec<dimmer_core::MonitorInfo> {
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|index| monitors.item(index)?.downcast::<gdk::Monitor>().ok())
        .enumerate()
        .map(|(index, monitor)| {
            let geometry = monitor.geometry();
            let label = monitor
                .description()
                .or_else(|| monitor.model())
                .unwrap_or_else(|| "Display".into());
            dimmer_core::MonitorInfo::new(
                monitor.connector().unwrap_or_default(),
                label,
                dimmer_core::Geometry {
                    x: geometry.x(),
                    y: geometry.y(),
                    width: geometry.width(),
                    height: geometry.height(),
                },
                f64::from(monitor.scale_factor()),
                index == 0,
                index,
            )
        })
        .collect()
}

fn update_banner(shared: &SharedState, banner: &adw::Banner, window: &adw::ApplicationWindow) {
    let snapshot: Snapshot = shared.snapshot();
    if snapshot.extension_connected {
        banner.set_revealed(false);
        return;
    }
    let _ = window;
    banner.set_button_label(Some("Details"));
    if snapshot.extension_installed {
        banner.set_title(
            "The Dimmer GNOME extension is installed but not connected. Log out and back in to load it.",
        );
    } else {
        banner.set_title(
            "The Dimmer GNOME extension is not installed. Dimming cannot draw over other apps until it is set up.",
        );
    }
    banner.set_revealed(true);
}

fn extension_help(shared: &SharedState, window: &adw::ApplicationWindow) {
    let snapshot = shared.snapshot();
    let is_flatpak = std::env::var_os("FLATPAK_ID").is_some();
    let body = if snapshot.extension_installed {
        "GNOME Shell loads newly installed extensions only at login. Log out, log back in, then open Dimmer. If it is still not connected, check the Extensions app and make sure \"Dimmer\" is enabled.\n\nOn Wayland, a normal application cannot draw over other apps, so the extension is required."
    } else if is_flatpak {
        "The GNOME Shell extension must be installed on the host because Flatpak apps cannot write to the host extension directory. From the Dimmer source checkout, run:\n\n    ./scripts/install-flatpak.sh\n\nOr download the extension ZIP from https://github.com/realrnvr/dimmer/releases and install it on the host. Then log out and back in and enable \"Dimmer\" in the Extensions app.\n\nOn Wayland, a normal application cannot draw over other apps, so the extension is required."
    } else {
        "Install the bundled GNOME Shell extension with:\n\n    dimmer --install-extension\n\n(If you are running from the source tree, scripts/install.sh also installs the application and extension.) Then log out and back in. The extension lives in ~/.local/share/gnome-shell/extensions/dimmer@dimmer.linux.\n\nOn Wayland, a normal application cannot draw over other apps, so the extension is required."
    };
    show_message(window, "GNOME extension required", body);
}

#[cfg(test)]
mod tests {
    use super::take_window_reset_result;
    use std::cell::Cell;

    #[test]
    fn reset_result_messages_only_apply_to_the_current_request() {
        let pending = Cell::new(Some(7));
        assert_eq!(take_window_reset_result(&pending, 6, true), None);
        assert_eq!(pending.get(), Some(7));
        assert_eq!(take_window_reset_result(&pending, 7, true), Some("Window reset succeeded"));
        assert_eq!(pending.get(), None);

        pending.set(Some(8));
        assert_eq!(take_window_reset_result(&pending, 8, false), Some("Window reset failed"));
        assert_eq!(pending.get(), None);
    }
}
