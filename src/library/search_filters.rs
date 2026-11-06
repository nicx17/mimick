use gettextrs::gettext;
use libadwaita::prelude::*;

pub struct FilterWidgets {
    pub filename_row: libadwaita::EntryRow,
    pub description_row: libadwaita::EntryRow,
    pub ocr_row: libadwaita::EntryRow,
    pub type_row: libadwaita::ComboRow,
    pub favorite_row: libadwaita::SwitchRow,
    pub motion_row: libadwaita::SwitchRow,
    pub not_in_album_row: libadwaita::SwitchRow,
    pub with_deleted_row: libadwaita::SwitchRow,
    pub visibility_row: libadwaita::ComboRow,
    pub rating_row: libadwaita::ComboRow,
    pub taken_after_row: libadwaita::EntryRow,
    pub taken_before_row: libadwaita::EntryRow,
    pub created_after_row: libadwaita::EntryRow,
    pub created_before_row: libadwaita::EntryRow,
    pub make_row: libadwaita::EntryRow,
    pub model_row: libadwaita::EntryRow,
    pub lens_row: libadwaita::EntryRow,
    pub country_row: libadwaita::EntryRow,
    pub state_row: libadwaita::EntryRow,
    pub city_row: libadwaita::EntryRow,
    pub tag_row: libadwaita::ComboRow,
    /// Tag ids matching `tag_row` entries after its leading "Any".
    pub tag_ids: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

pub(super) fn build_text_group() -> (
    libadwaita::PreferencesGroup,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Text"))
        .build();
    let filename = libadwaita::EntryRow::builder()
        .title(gettext("Filename contains"))
        .build();
    let desc = libadwaita::EntryRow::builder()
        .title(gettext("Description contains"))
        .build();
    let ocr = libadwaita::EntryRow::builder()
        .title(gettext("OCR text contains"))
        .build();
    group.add(&filename);
    group.add(&desc);
    group.add(&ocr);
    (group, filename, desc, ocr)
}

/// "Tags" group; the tag list is filled from the server by `set_available_tags`.
pub(super) fn build_tag_group() -> (libadwaita::PreferencesGroup, libadwaita::ComboRow) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Tags"))
        .build();
    let tag_row = combo_row(&gettext("Tag"), &[&gettext("Any")]);
    group.add(&tag_row);
    (group, tag_row)
}

fn combo_row(title: &str, items: &[&str]) -> libadwaita::ComboRow {
    libadwaita::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(items))
        .build()
}

/// The "Type and flags" group and its rows.
pub(super) struct FlagsGroup {
    pub group: libadwaita::PreferencesGroup,
    pub type_row: libadwaita::ComboRow,
    pub favorite: libadwaita::SwitchRow,
    pub motion: libadwaita::SwitchRow,
    pub not_in_album: libadwaita::SwitchRow,
    pub deleted: libadwaita::SwitchRow,
    pub visibility: libadwaita::ComboRow,
    pub rating: libadwaita::ComboRow,
}

pub(super) fn build_flags_group() -> FlagsGroup {
    fn switch(t: &str) -> libadwaita::SwitchRow {
        libadwaita::SwitchRow::builder().title(t).build()
    }

    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Type and flags"))
        .build();
    let type_row = combo_row(
        &gettext("Asset type"),
        &[
            &gettext("Any"),
            &gettext("Image only"),
            &gettext("Video only"),
        ],
    );
    let fav = switch(&gettext("Favourites only"));
    let motion = switch(&gettext("Motion photos only"));
    let not_album = switch(&gettext("Not in any album"));
    let deleted = switch(&gettext("Include deleted"));
    let vis = combo_row(
        &gettext("Visibility"),
        &[
            &gettext("Any"),
            &gettext("Timeline"),
            &gettext("Archive"),
            &gettext("Hidden"),
            &gettext("Locked"),
        ],
    );
    let rating = combo_row(
        &gettext("Minimum rating"),
        &[&gettext("Any"), "1+", "2+", "3+", "4+", "5"],
    );
    group.add(&type_row);
    group.add(&fav);
    group.add(&motion);
    group.add(&not_album);
    group.add(&deleted);
    group.add(&vis);
    group.add(&rating);
    FlagsGroup {
        group,
        type_row,
        favorite: fav,
        motion,
        not_in_album: not_album,
        deleted,
        visibility: vis,
        rating,
    }
}

pub(super) fn build_date_group() -> (
    libadwaita::PreferencesGroup,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Date range"))
        .description("e.g. 2024-01-15")
        .build();
    let taken_after = libadwaita::EntryRow::builder()
        .title(gettext("Taken after"))
        .build();
    let taken_before = libadwaita::EntryRow::builder()
        .title(gettext("Taken before"))
        .build();
    let created_after = libadwaita::EntryRow::builder()
        .title(gettext("Created after"))
        .build();
    let created_before = libadwaita::EntryRow::builder()
        .title(gettext("Created before"))
        .build();
    group.add(&taken_after);
    group.add(&taken_before);
    group.add(&created_after);
    group.add(&created_before);
    (
        group,
        taken_after,
        taken_before,
        created_after,
        created_before,
    )
}

pub(super) fn build_camera_group() -> (
    libadwaita::PreferencesGroup,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Camera"))
        .build();
    let make = libadwaita::EntryRow::builder()
        .title(gettext("Make"))
        .build();
    let model = libadwaita::EntryRow::builder()
        .title(gettext("Model"))
        .build();
    let lens = libadwaita::EntryRow::builder()
        .title(gettext("Lens model"))
        .build();
    group.add(&make);
    group.add(&model);
    group.add(&lens);
    (group, make, model, lens)
}

pub(super) fn build_location_group() -> (
    libadwaita::PreferencesGroup,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
    libadwaita::EntryRow,
) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Location"))
        .build();
    let country = libadwaita::EntryRow::builder()
        .title(gettext("Country"))
        .build();
    let state = libadwaita::EntryRow::builder()
        .title(gettext("State / region"))
        .build();
    let city = libadwaita::EntryRow::builder()
        .title(gettext("City"))
        .build();
    group.add(&country);
    group.add(&state);
    group.add(&city);
    (group, country, state, city)
}
