//! Dedicated search view for the library sidebar.
//!
//! The search form is a persistent `Revealer` that sits above the
//! content_stack.  When search is active the form stays visible and
//! results render in the normal masonry grid below it.  This avoids
//! the "search bar disappears" problem while keeping a single grid
//! renderer.

use gettextrs::gettext;
use std::cell::RefCell;
use std::rc::Rc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{LibraryFeatures, MetadataSearchFilters, Tag};
use crate::library::search_filters::{
    Choice, FilterWidgets, FlagsGroup, build_camera_group, build_date_group, build_flags_group,
    build_location_group, build_picks_group, build_text_group,
};
use crate::library::search_panel::{active_filter_count, build_side_panel, filters_label};

/// UI widgets for the dedicated search form.
///
/// `root` is a `gtk::Revealer` intended to sit between the controls
/// bar and the content_stack in the main layout.  When revealed, it
/// shows the search entry, mode selector, collapsible filters, and
/// action buttons.  Search results go into the existing grid.
pub struct SearchViewParts {
    /// Revealer wrapping the entire search form.  Inserted into the
    /// content pane above the content_stack.
    pub root: gtk::Revealer,
    pub search_entry: gtk::SearchEntry,
    pub search_mode: gtk::DropDown,
    pub search_button: gtk::Button,
    pub clear_button: gtk::Button,
    pub filters: FilterWidgets,
    /// Side panel holding the filter groups; docks on the right or slides over on phones.
    pub filter_panel: gtk::Box,
    pub filters_toggle: gtk::ToggleButton,
    filters_toggle_label: gtk::Label,
    pub panel_clear: gtk::Button,
    pub panel_search: gtk::Button,
    modes: Rc<RefCell<Vec<u32>>>,
}

const MODE_LABELS: [&str; 3] = ["Smart Search", "Filename", "OCR"];

pub fn selected_mode(view: &SearchViewParts) -> u32 {
    current_mode(&view.modes, &view.search_mode)
}

fn current_mode(modes: &RefCell<Vec<u32>>, dropdown: &gtk::DropDown) -> u32 {
    modes
        .borrow()
        .get(dropdown.selected() as usize)
        .copied()
        .unwrap_or(1)
}

pub fn set_available_modes(view: &SearchViewParts, smart: bool, ocr: bool) {
    let modes = available_modes(smart, ocr);
    if *view.modes.borrow() == modes {
        return;
    }
    let previous = selected_mode(view);
    let labels: Vec<&str> = modes
        .iter()
        .map(|mode| MODE_LABELS[*mode as usize])
        .collect();
    *view.modes.borrow_mut() = modes.clone();
    view.search_mode
        .set_model(Some(&gtk::StringList::new(&labels)));
    let index = modes.iter().position(|mode| *mode == previous).unwrap_or(0);
    view.search_mode.set_selected(index as u32);
}

fn available_modes(smart: bool, ocr: bool) -> Vec<u32> {
    [(0, smart), (1, true), (2, ocr)]
        .into_iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(mode, _)| mode)
        .collect()
}

/// Hide, and clear, the filters for what the server or account has turned off.
pub fn set_filter_features(view: &SearchViewParts, features: &LibraryFeatures) {
    let filters = &view.filters;
    filters.ocr_row.set_visible(features.ocr);
    if !features.ocr {
        filters.ocr_row.set_text("");
    }
    filters.rating_row.set_visible(features.ratings);
    if !features.ratings {
        filters.rating_row.set_selected(0);
    }
    for (pick, shown) in [
        (&filters.picks.tags, features.tags),
        (&filters.picks.people, features.people),
    ] {
        pick.row.set_visible(shown);
        if !shown {
            pick.clear();
        }
    }
}

fn build_search_bar() -> (gtk::Box, gtk::DropDown, gtk::SearchEntry) {
    let mode_model = gtk::StringList::new(&MODE_LABELS);
    let search_mode = gtk::DropDown::builder()
        .model(&mode_model)
        .selected(0)
        .hexpand(true)
        .tooltip_text(gettext(
            "Smart: CLIP-based semantic search.\n\
             Filename: matches against file names.\n\
             OCR: matches text inside images.",
        ))
        .build();

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Describe what you're looking for\u{2026}"))
        .build();

    let search_bar = gtk::Box::builder().spacing(6).build();
    search_bar.append(&search_mode);

    (search_bar, search_mode, search_entry)
}

struct ActionRow {
    /// Wraps onto a second line when the window is too narrow for one.
    root: libadwaita::WrapBox,
    filters_toggle: gtk::ToggleButton,
    filters_label: gtk::Label,
    clear: gtk::Button,
    search: gtk::Button,
}

fn build_action_row() -> ActionRow {
    let action_row = libadwaita::WrapBox::builder()
        .child_spacing(6)
        .line_spacing(6)
        .margin_top(2)
        .build();

    let filters_label = gtk::Label::new(Some(&filters_label(0)));
    let toggle_content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();
    toggle_content.append(&gtk::Image::from_icon_name("mimick-filter-symbolic"));
    toggle_content.append(&filters_label);
    let toggle_button = gtk::ToggleButton::builder()
        .child(&toggle_content)
        .tooltip_text(gettext("Show search filters"))
        .css_classes(["flat"])
        .build();

    let clear_button = gtk::Button::builder()
        .label(gettext("Clear"))
        .css_classes(vec!["flat".to_string()])
        .build();
    let search_button = gtk::Button::builder()
        .label(gettext("Search"))
        .css_classes(vec!["suggested-action".to_string()])
        .build();

    action_row.append(&toggle_button);
    action_row.append(&clear_button);

    ActionRow {
        root: action_row,
        filters_toggle: toggle_button,
        filters_label,
        clear: clear_button,
        search: search_button,
    }
}

/// The filter rows, and a column holding all their groups.
fn build_filter_panel() -> (FilterWidgets, gtk::Box) {
    let (text_group, filename_row, description_row, ocr_row) = build_text_group();
    let FlagsGroup {
        group: flags_group,
        type_row,
        favorite: fav,
        motion,
        not_in_album: not_album,
        deleted,
        visibility: vis,
        rating,
    } = build_flags_group();
    let (date_group, taken_after, taken_before, created_after, created_before) = build_date_group();
    let (camera_group, camera) = build_camera_group();
    let (loc_group, place) = build_location_group();
    let (picks_group, picks) = build_picks_group();

    let filter_box = stack_groups(&[
        &text_group,
        &picks_group,
        &flags_group,
        &date_group,
        &camera_group,
        &loc_group,
    ]);

    let widgets = FilterWidgets {
        filename_row,
        description_row,
        ocr_row,
        type_row,
        favorite_row: fav,
        motion_row: motion,
        not_in_album_row: not_album,
        with_deleted_row: deleted,
        visibility_row: vis,
        rating_row: rating,
        taken_after_row: taken_after,
        taken_before_row: taken_before,
        created_after_row: created_after,
        created_before_row: created_before,
        camera,
        place,
        picks,
    };
    (widgets, filter_box)
}

fn stack_groups(groups: &[&libadwaita::PreferencesGroup]) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(4)
        .build();
    for group in groups {
        column.append(*group);
    }
    column
}

/// Build the search form widget tree.
///
/// Returns a `SearchViewParts` whose `root` is a `Revealer`.  The
/// caller should insert `root` into the content pane above the
/// `content_stack` so the form stays visible while results load into
/// the grid.
pub fn build_search_view() -> SearchViewParts {
    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(4)
        .margin_start(8)
        .margin_end(8)
        .build();

    let (search_bar, search_mode, search_entry) = build_search_bar();
    let actions = build_action_row();
    search_bar.append(&actions.search);
    inner.append(&search_entry);
    inner.append(&search_bar);
    inner.append(&actions.root);

    let (filters, filter_box) = build_filter_panel();
    let panel = build_side_panel(&filter_box);
    let root = wrap_in_scrolled_revealer(&inner);
    close_panel_with_form(&actions.filters_toggle, &panel.close, &root);

    let modes: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(available_modes(true, true)));
    connect_mode_placeholder(&search_mode, &search_entry, modes.clone());

    SearchViewParts {
        root,
        search_entry,
        search_mode,
        search_button: actions.search,
        clear_button: actions.clear,
        filters,
        filter_panel: panel.root,
        filters_toggle: actions.filters_toggle,
        filters_toggle_label: actions.filters_label,
        panel_clear: panel.clear,
        panel_search: panel.search,
        modes,
    }
}

/// The panel's close button, and hiding the search form, both close the filters panel.
fn close_panel_with_form(toggle: &gtk::ToggleButton, close: &gtk::Button, form: &gtk::Revealer) {
    let toggle_for_close = toggle.clone();
    close.connect_clicked(move |_| toggle_for_close.set_active(false));
    let toggle = toggle.clone();
    form.connect_reveal_child_notify(move |form| {
        if !form.reveals_child() {
            toggle.set_active(false);
        }
    });
}

/// Placeholder text that describes what the chosen search mode looks for.
fn connect_mode_placeholder(
    search_mode: &gtk::DropDown,
    search_entry: &gtk::SearchEntry,
    modes: Rc<RefCell<Vec<u32>>>,
) {
    search_mode.connect_selected_notify(clone!(
        #[weak]
        search_entry,
        move |dd| {
            let ph = match current_mode(&modes, dd) {
                1 => &gettext("Search filenames"),
                2 => &gettext("Find words shown inside images"),
                _ => &gettext("Describe what you're looking for\u{2026}"),
            };
            search_entry.set_placeholder_text(Some(ph));
        }
    ));
}

fn wrap_in_scrolled_revealer(inner: &gtk::Box) -> gtk::Revealer {
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(320)
        .child(
            &libadwaita::Clamp::builder()
                .maximum_size(760)
                .tightening_threshold(560)
                .child(inner)
                .build(),
        )
        .build();
    scrolled
        .hscrollbar()
        .add_css_class("mimick-search-hscrollbar");

    gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(200)
        .reveal_child(false)
        .child(&scrolled)
        .build()
}

// ---------------------------------------------------------------------------
// Filter extraction helpers
// ---------------------------------------------------------------------------

/// What each position of the Asset type and Visibility rows sends; "Any" sends nothing.
const ASSET_TYPES: [Option<&str>; 3] = [None, Some("IMAGE"), Some("VIDEO")];
const VISIBILITIES: [Option<&str>; 5] = [
    None,
    Some("timeline"),
    Some("archive"),
    Some("hidden"),
    Some("locked"),
];

fn picked(values: &[Option<&str>], row: &libadwaita::ComboRow) -> Option<String> {
    values
        .get(row.selected() as usize)
        .copied()
        .flatten()
        .map(str::to_owned)
}

/// Collect all filter widget values into a `MetadataSearchFilters`.
pub fn collect_filters(view: &SearchViewParts) -> MetadataSearchFilters {
    MetadataSearchFilters {
        original_file_name: opt_string(&view.filters.filename_row.text()),
        description: opt_string(&view.filters.description_row.text()),
        ocr: opt_string(&view.filters.ocr_row.text()),
        asset_type: picked(&ASSET_TYPES, &view.filters.type_row),
        taken_after: normalise_iso_date(&view.filters.taken_after_row.text()),
        taken_before: normalise_iso_date(&view.filters.taken_before_row.text()),
        created_after: normalise_iso_date(&view.filters.created_after_row.text()),
        created_before: normalise_iso_date(&view.filters.created_before_row.text()),
        make: view.filters.camera.make.value(),
        model: view.filters.camera.model.value(),
        lens_model: view.filters.camera.lens.value(),
        country: view.filters.place.country.value(),
        state: view.filters.place.state.value(),
        city: view.filters.place.city.value(),
        person_ids: view.filters.picks.people.ids(),
        album_ids: view.filters.picks.albums.ids(),
        is_favorite: opt_true(view.filters.favorite_row.is_active()),
        is_motion: opt_true(view.filters.motion_row.is_active()),
        is_not_in_album: opt_true(view.filters.not_in_album_row.is_active()),
        with_deleted: opt_true(view.filters.with_deleted_row.is_active()),
        visibility: picked(&VISIBILITIES, &view.filters.visibility_row),
        tag_ids: view.filters.picks.tags.ids(),
        // The rows read Any, then 1+ to 5, so the position is the rating.
        rating: Some(view.filters.rating_row.selected()).filter(|stars| (1..=5).contains(stars)),
        ..Default::default()
    }
}

pub fn refresh_filter_count(view: &SearchViewParts) {
    let count = active_filter_count(&collect_filters(view));
    view.filters_toggle_label.set_label(&filters_label(count));
}

/// Reset the query and every filter.
pub fn clear_all_filters(view: &SearchViewParts) {
    view.search_entry.set_text("");
    clear_filter_rows(view);
}

/// Reset the filters, keeping the query.
pub fn clear_filter_rows(view: &SearchViewParts) {
    view.filters.filename_row.set_text("");
    view.filters.description_row.set_text("");
    view.filters.ocr_row.set_text("");
    view.filters.type_row.set_selected(0);
    view.filters.favorite_row.set_active(false);
    view.filters.motion_row.set_active(false);
    view.filters.not_in_album_row.set_active(false);
    view.filters.with_deleted_row.set_active(false);
    view.filters.visibility_row.set_selected(0);
    view.filters.rating_row.set_selected(0);
    view.filters.taken_after_row.set_text("");
    view.filters.taken_before_row.set_text("");
    view.filters.created_after_row.set_text("");
    view.filters.created_before_row.set_text("");
    let filters = &view.filters;
    for row in [
        &filters.camera.make,
        &filters.camera.model,
        &filters.camera.lens,
        &filters.place.country,
        &filters.place.state,
        &filters.place.city,
    ] {
        row.clear();
    }
    for pick in [
        &filters.picks.people,
        &filters.picks.tags,
        &filters.picks.albums,
    ] {
        pick.clear();
    }
    refresh_filter_count(view);
}

/// List the server's tags under Tags, keeping the ones picked that still exist.
pub fn set_available_tags(view: &SearchViewParts, tags: &[Tag]) {
    let choices = tags
        .iter()
        .map(|tag| Choice {
            id: tag.id.clone(),
            label: tag.value.clone(),
            icon: None,
        })
        .collect();
    view.filters.picks.tags.set_options(choices);
}

/// Pick only `tag_id` under Tags, if it is listed.
pub fn select_tag(view: &SearchViewParts, tag_id: &str) {
    view.filters.picks.tags.choose_only(tag_id);
}

/// Convert a non-empty trimmed string to `Some`, otherwise `None`.
pub fn opt_string(text: &gtk::glib::GString) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn opt_true(active: bool) -> Option<bool> {
    if active { Some(true) } else { None }
}

/// Normalise a user-entered date to ISO 8601 UTC. Accepts bare YYYY-MM-DD
/// or full RFC 3339.
pub fn normalise_iso_date(text: &gtk::glib::GString) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if chrono::DateTime::parse_from_rfc3339(trimmed).is_ok() {
        return Some(trimmed.to_string());
    }
    if chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").is_ok() {
        return Some(format!("{trimmed}T00:00:00.000Z"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_modes_keep_filename_search() {
        assert_eq!(available_modes(true, true), [0, 1, 2]);
        assert_eq!(available_modes(false, false), [1]);
        assert_eq!(available_modes(false, true), [1, 2]);
    }
}
