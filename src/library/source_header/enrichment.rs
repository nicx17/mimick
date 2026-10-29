//! Load source-specific facts and actions for the library header.

use std::future::Future;
use std::rc::Rc;

use gtk::prelude::*;

use crate::api_client::{LibraryAlbum, MetadataSearchFilters, SharedLinkTarget};

use super::super::LibraryWindowUi;
use super::super::dialogs::popover_menu;
use super::super::header_format::{album_details, count_label, owns_album, person_details};
use super::super::state::LibrarySource;
use super::super::tree_page::parent_path;
use super::{
    Leading, SourceHeader, add_action, render_facts, set_description, set_details, set_leading,
    set_title,
};

pub(super) fn enrich(ui: &Rc<LibraryWindowUi>, source: &LibrarySource) {
    match source {
        LibrarySource::Favorites => count_matching(
            ui,
            MetadataSearchFilters {
                is_favorite: Some(true),
                ..MetadataSearchFilters::default()
            },
        ),
        LibrarySource::Archive => count_matching(
            ui,
            MetadataSearchFilters {
                visibility: Some("archive".to_string()),
                ..MetadataSearchFilters::default()
            },
        ),
        LibrarySource::Place { city } => count_matching(
            ui,
            MetadataSearchFilters {
                city: Some(city.clone()),
                ..MetadataSearchFilters::default()
            },
        ),
        LibrarySource::AdvancedSearch { filters } => count_matching(ui, (**filters).clone()),
        LibrarySource::Thing { .. } => set_details(&ui.header, vec!["Things".to_string()]),
        LibrarySource::SmartSearch { .. } => {
            set_details(&ui.header, vec!["Smart search".to_string()])
        }
        LibrarySource::Folder { path } => {
            set_details(&ui.header, vec![path.clone()]);
        }
        LibrarySource::Album { id, .. }
        | LibrarySource::AlbumLocal { id, .. }
        | LibrarySource::AlbumUnified { id, .. } => enrich_album(ui, id.clone()),
        LibrarySource::Person { id, .. } => enrich_person(ui, id.clone()),
        LibrarySource::Tag { id, name } => enrich_tag(ui, id.clone(), name.clone()),
        LibrarySource::MapArea { ids, .. } => {
            *ui.header.count.borrow_mut() = Some(count_label(ids.0.len() as u64));
            render_facts(&ui.header);
            let ui_close = ui.clone();
            add_action(
                &ui.header,
                "window-close-symbolic",
                "Back to the map",
                move || super::super::navigation::close_map_area(&ui_close),
            );
        }
        _ => {}
    }
}

fn generation(ui: &LibraryWindowUi) -> u64 {
    ui.ctx.library_state.lock().generation
}

fn spawn_for_current<F, Fut>(ui: &Rc<LibraryWindowUi>, task: F)
where
    F: FnOnce(Rc<LibraryWindowUi>) -> Fut + 'static,
    Fut: Future<Output = Option<Box<dyn FnOnce(&Rc<LibraryWindowUi>)>>> + 'static,
{
    let ui = ui.clone();
    let started = generation(&ui);
    glib::MainContext::default().spawn_local(async move {
        let apply = task(ui.clone()).await;
        if let Some(apply) = apply
            && generation(&ui) == started
        {
            apply(&ui);
        }
    });
}

fn count_matching(ui: &Rc<LibraryWindowUi>, filters: MetadataSearchFilters) {
    spawn_for_current(ui, move |ui| async move {
        let total = ui.ctx.api_client.search_statistics(&filters).await.ok()?;
        Some(Box::new(move |ui: &Rc<LibraryWindowUi>| {
            *ui.header.count.borrow_mut() = Some(count_label(total));
            render_facts(&ui.header);
        }) as Box<dyn FnOnce(&Rc<LibraryWindowUi>)>)
    });
}

fn enrich_album(ui: &Rc<LibraryWindowUi>, album_id: String) {
    spawn_for_current(ui, move |ui| async move {
        let album = ui.ctx.api_client.fetch_album_detail(&album_id).await.ok()?;
        let me = super::super::ensure_current_user_id(&ui.ctx)
            .await
            .unwrap_or_default();
        Some(
            Box::new(move |ui: &Rc<LibraryWindowUi>| show_album(ui, album, &me))
                as Box<dyn FnOnce(&Rc<LibraryWindowUi>)>,
        )
    });
}

fn show_album(ui: &Rc<LibraryWindowUi>, album: LibraryAlbum, me: &str) {
    let header = &ui.header;
    set_title(ui, &album.album_name);
    *header.count.borrow_mut() = Some(count_label(album.asset_count.into()));
    set_details(header, album_details(&album, me));
    set_description(header, Some(&album.description));
    if owns_album(&album, me) {
        add_menu(
            header,
            super::super::album_edit::album_menu_items(ui, &album),
        );
        let (ui_share, share_album) = (ui.clone(), album.clone());
        add_action(header, "mimick-share-symbolic", "Share album", move || {
            super::super::album_share_dialog::present(ui_share.clone(), share_album.clone())
        });
    } else if !me.is_empty() {
        let (ui_leave, leave_album) = (ui.clone(), album.clone());
        let leave: Box<dyn Fn()> = Box::new(move || {
            super::super::album_assets::confirm_leave(&ui_leave, leave_album.clone())
        });
        add_menu(header, vec![("Leave Album…".to_string(), leave)]);
    }
    let (ui_zip, zip_id, zip_name) = (ui.clone(), album.id.clone(), album.album_name.clone());
    add_action(
        header,
        "mimick-download-symbolic",
        "Download album",
        move || {
            super::super::album_download::download_album(
                ui_zip.clone(),
                zip_id.clone(),
                zip_name.clone(),
            )
        },
    );
    let (ui_link, album_id) = (ui.clone(), album.id.clone());
    add_action(
        header,
        "insert-link-symbolic",
        "Create share link",
        move || {
            super::super::shared_link_dialog::present(
                ui_link.clone(),
                super::super::shared_link_dialog::LinkDialogMode::Create(SharedLinkTarget::Album(
                    album_id.clone(),
                )),
                Rc::new(|| {}),
            )
        },
    );
}

fn add_menu(header: &SourceHeader, items: Vec<(String, Box<dyn Fn()>)>) {
    let menu = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("More")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    popover_menu(&menu, items);
    header.actions.append(&menu);
}

fn enrich_person(ui: &Rc<LibraryWindowUi>, person_id: String) {
    spawn_for_current(ui, move |ui| async move {
        let person = ui.ctx.api_client.fetch_person(&person_id).await.ok()?;
        let count = ui.ctx.api_client.person_asset_count(&person_id).await.ok();
        Some(Box::new(move |ui: &Rc<LibraryWindowUi>| {
            let header = &ui.header;
            let name = if person.name.is_empty() {
                "Unnamed".to_string()
            } else {
                person.name.clone()
            };
            set_title(ui, &name);
            *header.count.borrow_mut() = count.map(count_label);
            let today = chrono::Local::now().date_naive();
            let mut details = person_details(person.birth_date.as_deref(), today);
            if person.is_hidden {
                details.push("Hidden".to_string());
            }
            set_details(header, details);
            let menu = gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .tooltip_text("More")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            add_favorite_toggle(ui, &person);
            popover_menu(
                &menu,
                super::super::people_view::person_menu_items(ui, &person),
            );
            header.actions.append(&menu);
        }) as Box<dyn FnOnce(&Rc<LibraryWindowUi>)>)
    });
}

fn add_favorite_toggle(ui: &Rc<LibraryWindowUi>, person: &crate::api_client::Person) {
    let favorite = person.is_favorite;
    let tooltip = if favorite {
        "Remove from favorites"
    } else {
        "Add to favorites"
    };
    let (ui_toggle, person) = (ui.clone(), person.clone());
    add_action(
        &ui.header,
        super::super::favorites::heart_icon(favorite),
        tooltip,
        move || super::super::people_view::set_favorite(&ui_toggle, &person, !favorite),
    );
}

fn enrich_tag(ui: &Rc<LibraryWindowUi>, tag_id: String, value: String) {
    let mut details = Vec::new();
    if let Some(parent) = parent_path(&value) {
        details.push(format!("In {}", parent.replace('/', " / ")));
    }
    set_details(&ui.header, details);
    count_matching(
        ui,
        MetadataSearchFilters {
            tag_ids: Some(vec![tag_id.clone()]),
            ..MetadataSearchFilters::default()
        },
    );
    spawn_for_current(ui, move |ui| async move {
        let tags = ui.ctx.api_client.fetch_tags().await.ok()?;
        let tag = tags.into_iter().find(|tag| tag.id == tag_id)?;
        Some(Box::new(move |ui: &Rc<LibraryWindowUi>| {
            let header = &ui.header;
            set_title(ui, &tag.name);
            set_leading(ui, Leading::TagColor(tag.color.clone()));
            let menu = gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .tooltip_text("More")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            popover_menu(&menu, super::super::tags_view::tag_menu_items(ui, &tag));
            header.actions.append(&menu);
        }) as Box<dyn FnOnce(&Rc<LibraryWindowUi>)>)
    });
}
