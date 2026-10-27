//! About page with port and original-project credits.

use adw::prelude::*;

use crate::ui::widgets::name_group_list;

const WEBSITE: &str = "https://www.nelsonpires.com/software/dimmer";
const DONATE: &str = "https://www.paypal.com/donate/?business=N9EWT4F7JBWFE&item_name=Thank+you+for+your+support&item_number=Dimmer+(App)&currency_code=USD";

/// The About tab.
pub struct AboutPage {
    page: adw::PreferencesPage,
}

impl AboutPage {
    #[must_use]
    pub fn new() -> Self {
        let page = adw::PreferencesPage::new();

        let header_group = adw::PreferencesGroup::new();
        name_group_list(&header_group, "Application information");
        let header = gtk::Box::new(gtk::Orientation::Vertical, 8);
        header.set_halign(gtk::Align::Center);
        header.set_margin_top(18);
        header.set_margin_bottom(18);
        let icon = gtk::Image::from_icon_name("video-display-symbolic");
        icon.set_pixel_size(96);
        icon.add_css_class("dimmer-about-icon");
        let title = gtk::Label::new(Some("Dimmer for Linux"));
        title.add_css_class("title-1");
        let version = gtk::Label::new(Some(&format!("Version {}", crate::APP_VERSION)));
        version.add_css_class("dim-label");
        let description = gtk::Label::new(Some(
            "Dim every display independently, with a master control, screen identities, boundaries and an optional tint.",
        ));
        description.set_wrap(true);
        description.set_justify(gtk::Justification::Center);
        description.set_max_width_chars(52);
        description.add_css_class("dim-label");
        header.append(&icon);
        header.append(&title);
        header.append(&version);
        header.append(&description);
        header_group.add(&header);
        page.add(&header_group);

        let links = adw::PreferencesGroup::new();
        links.set_title("Links");
        name_group_list(&links, "Links");
        let website = action_with_link(
            "Original Dimmer",
            "Nelson Pires' Windows application",
            WEBSITE,
            "Website",
        );
        let donate = action_with_link(
            "Donate",
            "Support the original Dimmer developer",
            DONATE,
            "Donate with PayPal",
        );
        links.add(&website);
        links.add(&donate);
        page.add(&links);

        let details = adw::PreferencesGroup::new();
        details.set_title("About this port");
        name_group_list(&details, "About this port");
        details.add(&info_row("Linux port", "Rust, GTK4/libadwaita and a GNOME Shell renderer"));
        details.add(&info_row("D-Bus protocol", &dimmer_core::PROTOCOL_VERSION.to_string()));
        details.add(&info_row(
            "Original",
            "Dimmer 2.0.1 © 2004–2022 Nelson Pires. Independent reimplementation, not affiliated with the original author.",
        ));
        details.add(&info_row(
            "Licence",
            "MIT for the Linux port. The original Windows application remains the property of its author.",
        ));
        details.add(&info_row(
            "Privacy",
            "No telemetry, no network access. Settings stay in your XDG config directory.",
        ));
        page.add(&details);

        Self { page }
    }

    #[must_use]
    pub const fn widget(&self) -> &adw::PreferencesPage {
        &self.page
    }
}

fn action_with_link(title: &str, subtitle: &str, uri: &str, label: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    let link = gtk::LinkButton::with_label(uri, label);
    link.set_valign(gtk::Align::Center);
    row.add_suffix(&link);
    row.set_activatable_widget(Some(&link));
    row
}

fn info_row(title: &str, subtitle: &str) -> adw::ActionRow {
    adw::ActionRow::builder().title(title).subtitle(subtitle).build()
}
