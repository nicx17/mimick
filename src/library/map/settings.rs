//! "Map Settings": which photos the Map page shows, saved in the config like Immich's.

use gettextrs::gettext;
use std::rc::Rc;

use chrono::{DateTime, Duration, Local, Months, Utc};
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::MarkerFilter;
use crate::config::{MapDateRange, MapFilters};
use crate::library::LibraryWindowUi;
use crate::library::day_row::{DayRow, day_start};

/// The marker query for `filters`, with relative ranges counted back from `now`.
pub(super) fn marker_filter(filters: &MapFilters, now: DateTime<Local>) -> MarkerFilter {
    let (created_after, created_before) = date_bounds(filters, now);
    MarkerFilter {
        favorites_only: filters.favorites_only,
        include_archived: filters.include_archived,
        with_partners: filters.with_partners,
        with_shared_albums: filters.with_shared_albums,
        created_after,
        created_before,
    }
}

fn date_bounds(
    filters: &MapFilters,
    now: DateTime<Local>,
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let back = |since: Option<DateTime<Local>>| (since.map(|t| t.with_timezone(&Utc)), None);
    match filters.date_range {
        MapDateRange::All => (None, None),
        MapDateRange::Day => back(Some(now - Duration::hours(24))),
        MapDateRange::Week => back(Some(now - Duration::days(7))),
        MapDateRange::Month => back(Some(now - Duration::days(30))),
        MapDateRange::Year => back(now.checked_sub_months(Months::new(12))),
        MapDateRange::ThreeYears => back(now.checked_sub_months(Months::new(36))),
        MapDateRange::Custom => (
            day_start(filters.custom_from.as_deref()),
            day_start(filters.custom_until.as_deref())
                .map(|start| start + Duration::days(1) - Duration::milliseconds(1)),
        ),
    }
}

fn range_label(range: MapDateRange) -> String {
    match range {
        MapDateRange::All => gettext("All Time"),
        MapDateRange::Day => gettext("Past 24 Hours"),
        MapDateRange::Week => gettext("Past 7 Days"),
        MapDateRange::Month => gettext("Past 30 Days"),
        MapDateRange::Year => gettext("Past Year"),
        MapDateRange::ThreeYears => gettext("Past 3 Years"),
        MapDateRange::Custom => gettext("Custom Range"),
    }
}

/// The rows of the dialog, read back into `MapFilters` when it closes.
struct Rows {
    favorites: libadwaita::SwitchRow,
    archived: libadwaita::SwitchRow,
    partners: libadwaita::SwitchRow,
    shared_albums: libadwaita::SwitchRow,
    range: libadwaita::ComboRow,
    from: DayRow,
    until: DayRow,
}

impl Rows {
    fn new(filters: &MapFilters) -> Self {
        let labels: Vec<String> = MapDateRange::ALL.into_iter().map(range_label).collect();
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let range = libadwaita::ComboRow::builder()
            .title(gettext("Taken"))
            .model(&gtk::StringList::new(&labels))
            .build();
        let selected = MapDateRange::ALL
            .iter()
            .position(|r| *r == filters.date_range);
        range.set_selected(selected.unwrap_or_default() as u32);
        Self {
            favorites: switch(&gettext("Only Favorites"), "", filters.favorites_only),
            archived: switch(
                &gettext("Include Archived"),
                &gettext("Your archived photos"),
                filters.include_archived,
            ),
            partners: switch(
                &gettext("Include Partners"),
                &gettext("Photos partners share with you"),
                filters.with_partners,
            ),
            shared_albums: switch(
                &gettext("Include Shared Albums"),
                &gettext("Photos in albums shared with you"),
                filters.with_shared_albums,
            ),
            range,
            from: DayRow::new(&gettext("From"), filters.custom_from.as_deref()),
            until: DayRow::new(&gettext("Until"), filters.custom_until.as_deref()),
        }
    }

    fn date_range(&self) -> MapDateRange {
        MapDateRange::ALL
            .get(self.range.selected() as usize)
            .copied()
            .unwrap_or_default()
    }

    fn filters(&self) -> MapFilters {
        MapFilters {
            favorites_only: self.favorites.is_active(),
            include_archived: self.archived.is_active(),
            with_partners: self.partners.is_active(),
            with_shared_albums: self.shared_albums.is_active(),
            date_range: self.date_range(),
            custom_from: self.from.day.borrow().clone(),
            custom_until: self.until.day.borrow().clone(),
        }
    }

    /// The From and Until rows show only for a custom range.
    fn sync_custom(&self) {
        let custom = self.date_range() == MapDateRange::Custom;
        self.from.row.set_visible(custom);
        self.until.row.set_visible(custom);
    }
}

fn switch(title: &str, subtitle: &str, active: bool) -> libadwaita::SwitchRow {
    let row = libadwaita::SwitchRow::builder()
        .title(title)
        .active(active)
        .build();
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    row
}

/// Open the dialog; closing it saves any change and reloads the markers.
pub(super) fn present(ui: &Rc<LibraryWindowUi>) {
    let saved = ui.ctx.config.read().data.library_map_filters.clone();
    let rows = Rc::new(Rows::new(&saved));
    let dialog = libadwaita::PreferencesDialog::builder()
        .title(gettext("Map Settings"))
        .content_width(420)
        .content_height(560)
        .build();
    dialog.add(&page(&rows));
    rows.sync_custom();
    let weak = Rc::downgrade(&rows);
    rows.range.connect_selected_notify(move |_| {
        if let Some(rows) = weak.upgrade() {
            rows.sync_custom();
        }
    });
    let ui_closed = ui.clone();
    dialog.connect_closed(move |_| {
        let filters = rows.filters();
        if filters != saved {
            apply(&ui_closed, filters);
        }
    });
    dialog.present(Some(&ui.window));
}

fn page(rows: &Rows) -> libadwaita::PreferencesPage {
    let show = libadwaita::PreferencesGroup::builder()
        .title(gettext("Show"))
        .build();
    show.add(&rows.favorites);
    show.add(&rows.archived);
    show.add(&rows.partners);
    show.add(&rows.shared_albums);
    let dates = libadwaita::PreferencesGroup::builder()
        .title(gettext("Date Range"))
        .build();
    dates.add(&rows.range);
    dates.add(&rows.from.row);
    dates.add(&rows.until.row);
    let page = libadwaita::PreferencesPage::new();
    page.add(&show);
    page.add(&dates);
    page
}

fn apply(ui: &Rc<LibraryWindowUi>, filters: MapFilters) {
    {
        let mut config = ui.ctx.config.write();
        config.data.library_map_filters = filters;
        if !config.save() {
            log::warn!("Could not save the map settings");
        }
    }
    super::controls::sync_settings_button(ui);
    super::view::reload(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Local)
    }

    #[test]
    fn default_settings_send_no_limits() {
        let filter = marker_filter(&MapFilters::default(), Local::now());
        assert_eq!(filter, MarkerFilter::default());
    }

    #[test]
    fn relative_ranges_count_back_from_now() {
        let now = at("2026-03-31T12:00:00Z");
        let past = |range| {
            let filters = MapFilters {
                date_range: range,
                ..MapFilters::default()
            };
            let filter = marker_filter(&filters, now);
            assert_eq!(filter.created_before, None);
            filter.created_after.unwrap().to_rfc3339()
        };
        assert_eq!(past(MapDateRange::Day), "2026-03-30T12:00:00+00:00");
        assert_eq!(past(MapDateRange::Month), "2026-03-01T12:00:00+00:00");
        assert_eq!(past(MapDateRange::ThreeYears), "2023-03-31T12:00:00+00:00");
    }

    #[test]
    fn custom_range_covers_whole_days_and_may_be_open() {
        let filters = MapFilters {
            date_range: MapDateRange::Custom,
            custom_from: Some("2024-05-01".into()),
            custom_until: Some("2024-05-03".into()),
            ..MapFilters::default()
        };
        let filter = marker_filter(&filters, Local::now());
        let (after, before) = (
            filter.created_after.unwrap(),
            filter.created_before.unwrap(),
        );
        assert_eq!(
            before - after,
            Duration::days(3) - Duration::milliseconds(1)
        );
        let open = MapFilters {
            custom_from: None,
            ..filters
        };
        assert_eq!(marker_filter(&open, Local::now()).created_after, None);
    }

    #[test]
    fn filters_left_unset_keep_immich_defaults() {
        let filters = MapFilters {
            favorites_only: true,
            with_partners: true,
            ..MapFilters::default()
        };
        let filter = marker_filter(&filters, Local::now());
        assert!(filter.favorites_only && filter.with_partners);
        assert!(!filter.include_archived && !filter.with_shared_albums);
    }
}
