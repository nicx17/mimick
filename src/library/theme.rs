//! Follow System, Light, or Dark, and a palette for each style (Warm White, Lavender, and
//! Mint when light; Black, Midnight, and Mocha when dark), kept in the config.

pub(crate) mod palettes;

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gettextrs::gettext;
use gtk::prelude::*;

use crate::app_context::AppContext;
use crate::i18n::gettext_noop;
use palettes::Palette;

/// Config value and label of each style, in menu order. Each value is also the CSS class
/// that paints its round button.
const SCHEMES: [(&str, &str); 3] = [
    ("system", gettext_noop("Follow System")),
    ("light", gettext_noop("Light")),
    ("dark", gettext_noop("Dark")),
];

/// Name of the main menu's custom slot the switcher fills.
pub(super) const MENU_SLOT: &str = "theme";

thread_local! {
    /// The light and dark palettes picked, by id.
    static PICKED: RefCell<(String, String)> = RefCell::default();
    static PROVIDER: OnceCell<gtk::CssProvider> = const { OnceCell::new() };
}

/// A change to the saved style; `None` keeps that part as it is.
#[derive(Default)]
pub struct Change<'a> {
    pub scheme: Option<&'a str>,
    pub light: Option<&'a str>,
    pub dark: Option<&'a str>,
}

/// Style the whole app by the saved scheme (anything unknown follows the system), with the
/// light or dark palette to match.
pub fn apply(scheme: &str, light: &str, dark: &str) {
    let color_scheme = match scheme {
        "light" => libadwaita::ColorScheme::ForceLight,
        "dark" => libadwaita::ColorScheme::ForceDark,
        _ => libadwaita::ColorScheme::Default,
    };
    let manager = libadwaita::StyleManager::default();
    manager.set_color_scheme(color_scheme);
    PICKED.set((light.to_string(), dark.to_string()));
    sync_palette(&manager);
}

/// Save and apply a change from the menu or Settings.
pub fn save(ctx: &AppContext, change: Change<'_>) {
    let mut config = ctx.config.write();
    let data = &mut config.data;
    let next = [
        change.scheme.unwrap_or(&data.color_scheme).to_string(),
        change.light.unwrap_or(&data.light_theme).to_string(),
        change.dark.unwrap_or(&data.dark_theme).to_string(),
    ];
    apply(&next[0], &next[1], &next[2]);
    let saved = [&data.color_scheme, &data.light_theme, &data.dark_theme];
    if saved.iter().zip(&next).all(|(saved, next)| *saved == next) {
        return;
    }
    let [scheme, light, dark] = next;
    (data.color_scheme, data.light_theme, data.dark_theme) = (scheme, light, dark);
    config.save();
}

/// Load the palette for the style showing now; empty for libadwaita's own.
fn sync_palette(manager: &libadwaita::StyleManager) {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    PROVIDER.with(|cell| {
        let provider = cell.get_or_init(|| install(&display, manager));
        let dark = manager.is_dark();
        let id = PICKED
            .with_borrow(|(light, dark_id)| if dark { dark_id.clone() } else { light.clone() });
        provider.load_from_string(&palettes::css(palettes::find(&id, dark)));
    });
}

/// The palette stylesheet, the swatches' colours, and following the system once it turns dark.
fn install(display: &gtk::gdk::Display, manager: &libadwaita::StyleManager) -> gtk::CssProvider {
    let priority = gtk::STYLE_PROVIDER_PRIORITY_APPLICATION;
    let swatches = gtk::CssProvider::new();
    swatches.load_from_string(&palettes::swatch_css());
    gtk::style_context_add_provider_for_display(display, &swatches, priority);
    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(display, &provider, priority + 1);
    manager.connect_dark_notify(sync_palette);
    provider
}

/// GNOME's style switcher: a round button per style, and under it a swatch per palette of
/// the style showing now. Each restyles the app at once and saves the choice.
pub(super) fn switcher(ctx: &Arc<AppContext>) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();
    let styles = choice_row(SCHEMES.iter().map(|&(name, label)| {
        let ctx = ctx.clone();
        let pick = move || {
            let change = Change {
                scheme: Some(name),
                ..Change::default()
            };
            save(&ctx, change);
        };
        Choice::new(name, &gettext(label), name.to_string(), pick)
    }));
    let light = palette_row(ctx, false);
    let dark = palette_row(ctx, true);
    column.append(&styles.row);
    column.append(&light.row);
    column.append(&dark.row);
    follow_style(&column, &light.row, &dark.row);
    let rows = Rc::new([styles, light, dark]);
    let ctx = ctx.clone();
    // Settings can change the style too, so ring the saved choices each time the menu opens.
    column.connect_map(move |_| sync_rows(&ctx, &rows));
    column
}

/// Swap the swatches as the style turns light or dark, even with the menu open.
fn follow_style(column: &gtk::Box, light: &gtk::Box, dark: &gtk::Box) {
    let manager = libadwaita::StyleManager::default();
    let (light, dark) = (light.downgrade(), dark.downgrade());
    let id = manager.connect_dark_notify(move |manager| {
        if let (Some(light), Some(dark)) = (light.upgrade(), dark.upgrade()) {
            light.set_visible(!manager.is_dark());
            dark.set_visible(manager.is_dark());
        }
    });
    let id = std::cell::Cell::new(Some(id));
    column.connect_destroy(move |_| {
        if let Some(id) = id.take() {
            manager.disconnect(id);
        }
    });
}

fn sync_rows(ctx: &AppContext, [styles, light, dark]: &[ChoiceRow; 3]) {
    let dark_now = libadwaita::StyleManager::default().is_dark();
    light.row.set_visible(!dark_now);
    dark.row.set_visible(dark_now);
    // Copied out first: ringing a button saves, which needs the config unlocked.
    let saved = {
        let data = &ctx.config.read().data;
        [&data.color_scheme, &data.light_theme, &data.dark_theme].map(String::clone)
    };
    styles.ring(&saved[0]);
    light.ring(&saved[1]);
    dark.ring(&saved[2]);
}

fn palette_row(ctx: &Arc<AppContext>, dark: bool) -> ChoiceRow {
    choice_row(palettes::of_style(dark).map(|palette: &'static Palette| {
        let ctx = ctx.clone();
        let pick = move || {
            let change = if palette.dark {
                Change {
                    dark: Some(palette.id),
                    ..Change::default()
                }
            } else {
                Change {
                    light: Some(palette.id),
                    ..Change::default()
                }
            };
            save(&ctx, change);
        };
        let class = palettes::swatch_class(palette);
        Choice::new(
            &class,
            &gettext(palette.label),
            palette.id.to_string(),
            pick,
        )
    }))
}

/// One round button: its CSS class, label, the value it saves, and saving it.
struct Choice {
    button: gtk::CheckButton,
    value: String,
}

impl Choice {
    fn new(class: &str, label: &str, value: String, pick: impl Fn() + 'static) -> Self {
        let button = gtk::CheckButton::builder()
            .tooltip_text(label)
            .css_classes([class])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(label)]);
        button.connect_toggled(move |button| {
            if button.is_active() {
                pick();
            }
        });
        Self { button, value }
    }
}

/// A row of grouped round buttons, one of them ringed.
struct ChoiceRow {
    row: gtk::Box,
    choices: Vec<Choice>,
}

impl ChoiceRow {
    /// Ring the choice saving `value`, or the first when none does.
    fn ring(&self, value: &str) {
        let choice = self.choices.iter().find(|choice| choice.value == value);
        if let Some(choice) = choice.or(self.choices.first())
            && !choice.button.is_active()
        {
            choice.button.set_active(true);
        }
    }
}

fn choice_row(choices: impl Iterator<Item = Choice>) -> ChoiceRow {
    let row = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::Center)
        .css_classes(["mimick-style-switcher"])
        .build();
    let choices: Vec<Choice> = choices.collect();
    for choice in &choices {
        if let Some(first) = choices
            .first()
            .filter(|first| first.button != choice.button)
        {
            choice.button.set_group(Some(&first.button));
        }
        row.append(&choice.button);
    }
    ChoiceRow { row, choices }
}
