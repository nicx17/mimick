//! Appearance: the app's style, and the palette for each of its light and dark styles.

use std::sync::Arc;

use adw::prelude::*;
use gettextrs::gettext;
use libadwaita as adw;

use crate::app_context::AppContext;
use crate::library::theme::{self, Change, palettes};

/// Config value of each Style row choice, in row order.
const SCHEMES: [&str; 3] = ["system", "light", "dark"];

/// Adds the Appearance group, showing and saving the same settings as the menu's switcher.
pub(super) fn build_appearance_group(page: &adw::PreferencesPage, ctx: &Arc<AppContext>) {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Appearance"))
        .build();
    page.add(&group);
    let labels = [gettext("Follow System"), gettext("Light"), gettext("Dark")];
    let style = combo(&gettext("Style"), &labels);
    let light = palette_row(&gettext("Light Theme"), false);
    let dark = palette_row(&gettext("Dark Theme"), true);
    {
        let data = &ctx.config.read().data;
        select(&style, SCHEMES.iter().position(|s| *s == data.color_scheme));
        select(&light, palette_index(&data.light_theme, false));
        select(&dark, palette_index(&data.dark_theme, true));
    }
    save_on_change(ctx, &style, |i| Change {
        scheme: SCHEMES.get(i as usize).copied(),
        ..Change::default()
    });
    save_on_change(ctx, &light, |i| Change {
        light: palette_id(i, false),
        ..Change::default()
    });
    save_on_change(ctx, &dark, |i| Change {
        dark: palette_id(i, true),
        ..Change::default()
    });
    group.add(&style);
    group.add(&light);
    group.add(&dark);
}

/// Picking another entry in `row` saves the change it stands for.
fn save_on_change(ctx: &Arc<AppContext>, row: &adw::ComboRow, change: fn(u32) -> Change<'static>) {
    let ctx = ctx.clone();
    row.connect_selected_notify(move |row| theme::save(&ctx, change(row.selected())));
}

fn combo(title: &str, labels: &[String]) -> adw::ComboRow {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&labels))
        .build()
}

fn palette_row(title: &str, dark: bool) -> adw::ComboRow {
    let labels: Vec<String> = palettes::of_style(dark)
        .map(|palette| gettext(palette.label))
        .collect();
    combo(title, &labels)
}

fn select(row: &adw::ComboRow, index: Option<usize>) {
    row.set_selected(index.unwrap_or(0) as u32);
}

fn palette_index(id: &str, dark: bool) -> Option<usize> {
    palettes::of_style(dark).position(|palette| palette.id == id)
}

fn palette_id(index: u32, dark: bool) -> Option<&'static str> {
    palettes::of_style(dark)
        .nth(index as usize)
        .map(|palette| palette.id)
}
