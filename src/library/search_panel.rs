//! The search filters side panel: a titled, scrolling column of filter groups with
//! Reset and Search, plus the active-filter count shown on the Filters button.

use gtk::prelude::*;

use crate::api_client::MetadataSearchFilters;

pub(super) struct SidePanel {
    pub root: gtk::Box,
    pub close: gtk::Button,
    pub clear: gtk::Button,
    pub search: gtk::Button,
}

/// "Filters" title with a close button, the scrolling groups, then Reset and Search.
pub(super) fn build_side_panel(filter_box: &gtk::Box) -> SidePanel {
    let (header, close) = panel_header();
    filter_box.set_margin_start(12);
    filter_box.set_margin_end(12);
    filter_box.set_margin_bottom(12);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::External)
        .vexpand(true)
        .child(filter_box)
        .build();
    let (footer, clear, search) = panel_footer();
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["mimick-filters-pane"])
        .build();
    root.append(&header);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&scroller);
    root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    root.append(&footer);
    SidePanel {
        root,
        close,
        clear,
        search,
    }
}

fn panel_header() -> (gtk::Box, gtk::Button) {
    let title = gtk::Label::builder()
        .label("Filters")
        .xalign(0.0)
        .hexpand(true)
        .css_classes(["title-4"])
        .build();
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close filters")
        .css_classes(["flat", "circular"])
        .build();
    let header = gtk::Box::builder()
        .spacing(6)
        .margin_top(10)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(8)
        .build();
    header.append(&title);
    header.append(&close);
    (header, close)
}

fn panel_footer() -> (gtk::Box, gtk::Button, gtk::Button) {
    let clear = gtk::Button::builder()
        .label("Reset")
        .tooltip_text("Clear every filter, keeping the search text")
        .hexpand(true)
        .build();
    let search = gtk::Button::builder()
        .label("Search")
        .hexpand(true)
        .css_classes(["suggested-action"])
        .build();
    let footer = gtk::Box::builder()
        .spacing(8)
        .margin_top(8)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    footer.append(&clear);
    footer.append(&search);
    (footer, clear, search)
}

/// How many filters are set; the sort order isn't a filter.
pub fn active_filter_count(filters: &MetadataSearchFilters) -> usize {
    match serde_json::to_value(filters) {
        Ok(serde_json::Value::Object(fields)) => {
            fields.keys().filter(|key| *key != "order").count()
        }
        _ => 0,
    }
}

pub fn filters_label(count: usize) -> String {
    if count == 0 {
        "Filters".to_string()
    } else {
        format!("Filters · {count}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_count_ignores_sort_order() {
        assert_eq!(active_filter_count(&MetadataSearchFilters::default()), 0);
        let filters = MetadataSearchFilters {
            is_favorite: Some(true),
            city: Some("Lisbon".into()),
            order: Some(crate::api_client::SortOrder::Asc),
            ..MetadataSearchFilters::default()
        };
        assert_eq!(active_filter_count(&filters), 2);
    }

    #[test]
    fn filters_label_shows_the_count() {
        assert_eq!(filters_label(0), "Filters");
        assert_eq!(filters_label(3), "Filters · 3");
    }
}
