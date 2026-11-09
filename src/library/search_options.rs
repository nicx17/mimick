//! What the search filters can pick from: people, albums, and the camera and place values
//! Immich has, with models, lenses, states, and cities narrowed by the rows above them.

use std::rc::{Rc, Weak};

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::{Person, SuggestionContext, SuggestionField};

use super::LibraryWindowUi;
use super::search_filters::{Choice, FilterWidgets, SuggestRow};

const AVATAR_SIZE: i32 = 32;

/// Each row, and the rows to load again when it changes.
const NARROWS: [(SuggestionField, &[SuggestionField]); 4] = [
    (
        SuggestionField::CameraMake,
        &[SuggestionField::CameraModel, SuggestionField::LensModel],
    ),
    (SuggestionField::CameraModel, &[SuggestionField::LensModel]),
    (
        SuggestionField::Country,
        &[SuggestionField::State, SuggestionField::City],
    ),
    (SuggestionField::State, &[SuggestionField::City]),
];

const ALL_FIELDS: [SuggestionField; 6] = [
    SuggestionField::CameraMake,
    SuggestionField::CameraModel,
    SuggestionField::LensModel,
    SuggestionField::Country,
    SuggestionField::State,
    SuggestionField::City,
];

fn row_for(filters: &FilterWidgets, field: SuggestionField) -> &SuggestRow {
    match field {
        SuggestionField::CameraMake => &filters.camera.make,
        SuggestionField::CameraModel => &filters.camera.model,
        SuggestionField::LensModel => &filters.camera.lens,
        SuggestionField::Country => &filters.place.country,
        SuggestionField::State => &filters.place.state,
        SuggestionField::City => &filters.place.city,
    }
}

/// Only the choices that narrow `field`, so a change elsewhere doesn't make its answer stale.
fn context_for(filters: &FilterWidgets, field: SuggestionField) -> SuggestionContext {
    let camera = matches!(
        field,
        SuggestionField::CameraModel | SuggestionField::LensModel
    );
    let place = matches!(field, SuggestionField::State | SuggestionField::City);
    SuggestionContext {
        make: filters.camera.make.value().filter(|_| camera),
        model: filters
            .camera
            .model
            .value()
            .filter(|_| field == SuggestionField::LensModel),
        country: filters.place.country.value().filter(|_| place),
        state: filters
            .place
            .state
            .value()
            .filter(|_| field == SuggestionField::City),
    }
}

/// Reload the narrower rows whenever a row above them changes.
pub(super) fn connect(ui: &Rc<LibraryWindowUi>) {
    for (field, narrowed) in NARROWS {
        let weak = Rc::downgrade(ui);
        row_for(&ui.search_view.filters, field)
            .row
            .connect_selected_notify(move |_| {
                if let Some(ui) = weak.upgrade() {
                    for field in narrowed {
                        load_suggestions(&ui, *field);
                    }
                }
            });
    }
}

/// Fill every list the filters pick from; called when Search opens.
pub(super) fn refresh(ui: &Rc<LibraryWindowUi>) {
    for field in ALL_FIELDS {
        load_suggestions(ui, field);
    }
    load_albums(ui);
    if ui.features.get().people {
        load_people(ui);
    }
}

fn load_suggestions(ui: &Rc<LibraryWindowUi>, field: SuggestionField) {
    let context = context_for(&ui.search_view.filters, field);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let values = ui
            .ctx
            .api_client
            .fetch_search_suggestions(field, &context)
            .await;
        let filters = &ui.search_view.filters;
        match values {
            Ok(values) if context_for(filters, field) == context => {
                row_for(filters, field).set_values(values);
            }
            Ok(_) => {}
            Err(err) => log::debug!("No search suggestions for {field:?}: {err}"),
        }
    });
}

fn load_albums(ui: &Rc<LibraryWindowUi>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let cached = ui.ctx.library_state.lock().albums.clone();
        let albums = if cached.is_empty() {
            match ui.ctx.api_client.fetch_library_albums().await {
                Ok(albums) => albums,
                Err(err) => return log::debug!("No albums for search: {err}"),
            }
        } else {
            cached
        };
        let mut choices: Vec<Choice> = albums
            .into_iter()
            .map(|album| Choice {
                id: album.id,
                label: album.album_name,
                icon: None,
            })
            .collect();
        choices.sort_by_key(|choice| choice.label.to_lowercase());
        ui.search_view.filters.picks.albums.set_options(choices);
    });
}

/// Named people with their faces; people without a name can't be told apart in a list.
fn load_people(ui: &Rc<LibraryWindowUi>) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let people = match ui.ctx.api_client.fetch_people(false).await {
            Ok(people) => people,
            Err(err) => return log::debug!("No people for search: {err}"),
        };
        let weak = Rc::downgrade(&ui);
        let mut choices: Vec<Choice> = people
            .into_iter()
            .filter(|person| !person.name.trim().is_empty())
            .map(|person| person_choice(&weak, person))
            .collect();
        choices.sort_by_key(|choice| choice.label.to_lowercase());
        ui.search_view.filters.picks.people.set_options(choices);
    });
}

/// The face is drawn when the checklist opens; a weak handle keeps the window from leaking.
fn person_choice(ui: &Weak<LibraryWindowUi>, person: Person) -> Choice {
    let (ui, id, label) = (ui.clone(), person.id.clone(), person.name.clone());
    Choice {
        id,
        label,
        icon: Some(Rc::new(move || match ui.upgrade() {
            Some(ui) => super::people_view::avatar(&ui, &person, AVATAR_SIZE).upcast(),
            None => libadwaita::Avatar::new(AVATAR_SIZE, Some(&person.name), true).upcast(),
        })),
    }
}
