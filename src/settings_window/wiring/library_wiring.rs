use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use gtk::prelude::*;
use libadwaita as adw;

use crate::app_context::AppContext;
use crate::config::DateHeadings;
use crate::settings_window::library::LibraryWidgets;

use super::{grid_quality_index, grid_quality_value, save_flag};
use crate::settings_window::show_alert;

pub(crate) fn connect_library_rows(
    ctx: &Arc<AppContext>,
    library: &LibraryWidgets,
    library_view_row: &adw::SwitchRow,
) {
    let enabled = ctx.config.read().data.library_view_enabled;
    for group in &library.library_groups {
        group.set_visible(enabled);
    }
    let (c, groups) = (ctx.clone(), library.library_groups.clone());
    library_view_row.connect_active_notify(move |row| {
        let active = row.is_active();
        for group in &groups {
            group.set_visible(active);
        }
        save_flag(&c, |d| {
            std::mem::replace(&mut d.library_view_enabled, active) != active
        });
    });
    let c = ctx.clone();
    library.preview_full_row.connect_active_notify(move |row| {
        let active = row.is_active();
        save_flag(&c, |d| {
            std::mem::replace(&mut d.library_preview_full_resolution, active) != active
        });
    });
    library.grid_quality_row.set_selected(grid_quality_index(
        &ctx.config.read().data.library_grid_quality,
    ));
    let c = ctx.clone();
    library
        .grid_quality_row
        .connect_selected_notify(move |row| {
            let value = grid_quality_value(row.selected());
            save_flag(&c, |d| {
                let changed = d.library_grid_quality != value;
                d.library_grid_quality = value.to_string();
                changed
            });
            c.publish_grid_style();
        });
    connect_date_headings_row(ctx, &library.date_headings_row);
    connect_raw_rows(ctx, library);
    connect_disk_cache_row(ctx, &library.disk_cache_row);
    connect_clear_cache(ctx, library);
    refresh_cache_usage(&library.disk_cache_row);
}

/// Measure the caches off the GTK thread and show it under the cache limit.
fn refresh_cache_usage(row: &adw::SpinRow) {
    let row = row.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let used = tokio::task::spawn_blocking(crate::cache_manager::total_size_blocking)
            .await
            .unwrap_or(0);
        if let Some(row) = row.upgrade() {
            row.set_subtitle(&crate::settings_window::library::cache_usage_label(
                used,
                row.value() as u32,
            ));
        }
    });
}

fn connect_clear_cache(ctx: &Arc<AppContext>, library: &LibraryWidgets) {
    let (thumbnail_cache, row) = (ctx.thumbnail_cache.clone(), library.disk_cache_row.clone());
    library.clear_cache_btn.connect_clicked(move |btn| {
        let _ = thumbnail_cache.clear();
        btn.set_sensitive(false);
        let (btn, row) = (btn.clone(), row.clone());
        glib::MainContext::default().spawn_local(async move {
            let result = tokio::task::spawn_blocking(crate::cache_manager::clear_all_blocking)
                .await
                .map_err(|err| err.to_string())
                .and_then(|inner| inner);
            btn.set_sensitive(true);
            if let Err(err) = result
                && let Some(window) = btn.root().and_downcast::<gtk::Window>()
            {
                show_alert(&window, "Could Not Clear Cache", &err);
            }
            refresh_cache_usage(&row);
        });
    });
}

fn connect_date_headings_row(ctx: &Arc<AppContext>, row: &adw::ComboRow) {
    let current = ctx.config.read().data.library_date_headings;
    let index = DateHeadings::ALL.iter().position(|mode| *mode == current);
    row.set_selected(index.unwrap_or_default() as u32);
    let ctx = ctx.clone();
    row.connect_selected_notify(move |row| {
        let mode = DateHeadings::ALL
            .get(row.selected() as usize)
            .copied()
            .unwrap_or_default();
        save_flag(&ctx, |d| {
            std::mem::replace(&mut d.library_date_headings, mode) != mode
        });
        ctx.date_headings.send_replace(mode);
    });
}

fn connect_raw_rows(ctx: &Arc<AppContext>, library: &LibraryWidgets) {
    library
        .raw_cache_row
        .set_sensitive(library.raw_full_decode_row.is_active());
    let (c, cache_row) = (ctx.clone(), library.raw_cache_row.clone());
    library
        .raw_full_decode_row
        .connect_active_notify(move |row| {
            let active = row.is_active();
            cache_row.set_sensitive(active);
            crate::library::set_raw_full_decode(active);
            save_flag(&c, |d| {
                std::mem::replace(&mut d.raw_full_decode, active) != active
            });
        });
    let c = ctx.clone();
    library.raw_cache_row.connect_active_notify(move |row| {
        let active = row.is_active();
        crate::library::set_raw_cache_enabled(active);
        save_flag(&c, |d| {
            std::mem::replace(&mut d.raw_decode_cache_enabled, active) != active
        });
    });
}

/// Debounced: the spin row fires on every step, so save 400ms after the last change.
fn connect_disk_cache_row(ctx: &Arc<AppContext>, row: &adw::SpinRow) {
    let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    let ctx = ctx.clone();
    row.connect_value_notify(move |row| {
        let new_value = row.value() as u32;
        if let Some(id) = pending.take() {
            id.remove();
        }
        let (ctx, pending_for_save, row) = (ctx.clone(), pending.clone(), row.clone());
        let id = glib::timeout_add_local_once(Duration::from_millis(400), move || {
            pending_for_save.set(None);
            refresh_cache_usage(&row);
            save_flag(&ctx, |d| {
                std::mem::replace(&mut d.cache_disk_cap_mb, new_value) != new_value
            });
        });
        pending.set(Some(id));
    });
}
