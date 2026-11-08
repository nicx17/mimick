use gettextrs::gettext;
use libadwaita::prelude::*;
use std::rc::Rc;

mod choices;
mod suggest;

pub use choices::{Choice, MultiChoice};
pub use suggest::SuggestRow;

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
    pub camera: CameraRows,
    pub place: PlaceRows,
    pub picks: Picks,
}

/// Make, model, and lens; models and lenses narrow to the make (and model) chosen.
pub struct CameraRows {
    pub make: SuggestRow,
    pub model: SuggestRow,
    pub lens: SuggestRow,
}

/// Country, state, and city; each narrows the ones below it.
pub struct PlaceRows {
    pub country: SuggestRow,
    pub state: SuggestRow,
    pub city: SuggestRow,
}

/// People, tags, and albums a photo must all have.
pub struct Picks {
    pub people: Rc<MultiChoice>,
    pub tags: Rc<MultiChoice>,
    pub albums: Rc<MultiChoice>,
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

/// People, tags, and albums; their lists are filled from the server when Search opens.
pub(super) fn build_picks_group() -> (libadwaita::PreferencesGroup, Picks) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("People, Tags, and Albums"))
        .description(gettext("Photos have to have everything you pick"))
        .build();
    let picks = Picks {
        people: MultiChoice::new(&gettext("People"), &gettext("Anyone")),
        tags: MultiChoice::new(&gettext("Tags"), &gettext("Any")),
        albums: MultiChoice::new(&gettext("Albums"), &gettext("Any")),
    };
    group.add(&picks.people.row);
    group.add(&picks.tags.row);
    group.add(&picks.albums.row);
    (group, picks)
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

/// The values Immich has for each row are filled in when Search opens.
pub(super) fn build_camera_group() -> (libadwaita::PreferencesGroup, CameraRows) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Camera"))
        .build();
    let rows = CameraRows {
        make: SuggestRow::new(&gettext("Make")),
        model: SuggestRow::new(&gettext("Model")),
        lens: SuggestRow::new(&gettext("Lens model")),
    };
    group.add(&rows.make.row);
    group.add(&rows.model.row);
    group.add(&rows.lens.row);
    (group, rows)
}

pub(super) fn build_location_group() -> (libadwaita::PreferencesGroup, PlaceRows) {
    let group = libadwaita::PreferencesGroup::builder()
        .title(gettext("Location"))
        .build();
    let rows = PlaceRows {
        country: SuggestRow::new(&gettext("Country")),
        state: SuggestRow::new(&gettext("State / region")),
        city: SuggestRow::new(&gettext("City")),
    };
    group.add(&rows.country.row);
    group.add(&rows.state.row);
    group.add(&rows.city.row);
    (group, rows)
}
