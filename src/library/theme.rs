//! Follow System, Light, or Dark, picked from the main menu and kept in the config.

use std::sync::Arc;

use gettextrs::gettext;
use gtk::prelude::*;

use crate::app_context::AppContext;
use crate::i18n::gettext_noop;

/// Config value and label of each choice, in menu order. Each value is also the CSS class
/// that paints its round button.
const SCHEMES: [(&str, &str); 3] = [
    ("system", gettext_noop("Follow System")),
    ("light", gettext_noop("Light")),
    ("dark", gettext_noop("Dark")),
];

/// Name of the main menu's custom slot the switcher fills.
pub(super) const MENU_SLOT: &str = "theme";

/// Style the whole app by a saved config value; anything unknown follows the system.
pub fn apply(name: &str) {
    let scheme = match name {
        "light" => libadwaita::ColorScheme::ForceLight,
        "dark" => libadwaita::ColorScheme::ForceDark,
        _ => libadwaita::ColorScheme::Default,
    };
    libadwaita::StyleManager::default().set_color_scheme(scheme);
}

/// GNOME's style switcher: a round button per style, the picked one ringed. Each restyles
/// the app at once and saves the choice.
pub(super) fn switcher(ctx: &Arc<AppContext>) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(18)
        .halign(gtk::Align::Center)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .css_classes(["mimick-style-switcher"])
        .build();
    let saved = ctx.config.read().data.color_scheme.clone();
    // Anything unknown follows the system, as `apply` does.
    let current = SCHEMES
        .iter()
        .find(|(name, _)| *name == saved)
        .map_or("system", |(name, _)| *name);
    let mut first: Option<gtk::CheckButton> = None;
    for (name, label) in SCHEMES {
        let button = scheme_button(ctx, name, &gettext(label), name == current);
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        row.append(&button);
    }
    row
}

fn scheme_button(
    ctx: &Arc<AppContext>,
    name: &'static str,
    label: &str,
    active: bool,
) -> gtk::CheckButton {
    let button = gtk::CheckButton::builder()
        .tooltip_text(label)
        .active(active)
        .css_classes([name])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    let ctx = ctx.clone();
    button.connect_toggled(move |button| {
        if !button.is_active() {
            return;
        }
        apply(name);
        let mut config = ctx.config.write();
        if config.data.color_scheme != name {
            config.data.color_scheme = name.to_string();
            config.save();
        }
    });
    button
}
