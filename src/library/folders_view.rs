use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use libadwaita::prelude::*;

use crate::api_client::{LibraryAsset, SortOrder};

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::state::{LibraryPage, LibrarySource, Location};
use super::tree_page::{
    TreePage, build_tree_page, leaf_name, parent_path, render_breadcrumbs, show_empty_state,
};

pub(super) struct FoldersView {
    pub page: TreePage,
    asset_folders: RefCell<HashSet<String>>,
    folders: RefCell<BTreeSet<String>>,
    path: RefCell<Option<String>>,
    opened: Cell<bool>,
}

pub(super) fn build_folders_view() -> FoldersView {
    FoldersView {
        page: build_tree_page("folder-symbolic"),
        asset_folders: RefCell::default(),
        folders: RefCell::default(),
        path: RefCell::default(),
        opened: Cell::new(false),
    }
}

pub(super) fn show(ui: &Rc<LibraryWindowUi>, path: Option<String>) {
    *ui.folders.path.borrow_mut() = path;
    ui.content_stack.set_visible_child_name("folders");
    render(ui);
    reload(ui.clone());
}

fn reload(ui: Rc<LibraryWindowUi>) {
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.fetch_folder_paths().await {
            Ok(paths) => {
                let view = &ui.folders;
                *view.folders.borrow_mut() = all_folders(&paths);
                *view.asset_folders.borrow_mut() = paths.into_iter().collect();
                if !view.opened.replace(true) && view.path.borrow().is_none() {
                    let start = start_path(&view.folders.borrow(), &view.asset_folders.borrow());
                    *view.path.borrow_mut() = start.clone();
                    let mut state = ui.ctx.library_state.lock();
                    if state.location == Location::Page(LibraryPage::Folders { path: None }) {
                        state.location = Location::Page(LibraryPage::Folders { path: start });
                    }
                }
                render(&ui);
            }
            Err(err) => show_alert_dialog(
                &ui,
                "Could Not Load Folders",
                &format!("{err}\n\nYour API key needs the 'folder.read' permission."),
            ),
        }
    });
}

fn open_path(ui: &Rc<LibraryWindowUi>, path: Option<String>) {
    super::navigation::navigate(ui, Location::Page(LibraryPage::Folders { path }));
}

fn open_photos(ui: &Rc<LibraryWindowUi>, path: &str) {
    super::navigation::navigate_source(
        ui,
        LibrarySource::Folder {
            path: path.to_string(),
        },
    );
}

fn render(ui: &Rc<LibraryWindowUi>) {
    let view = &ui.folders;
    let folders = view.folders.borrow().clone();
    let path = view
        .path
        .borrow()
        .clone()
        .filter(|path| folders.contains(path));
    *view.path.borrow_mut() = path.clone();
    let ui_crumb = ui.clone();
    render_breadcrumbs(
        &view.page,
        "Folders",
        path.as_deref(),
        Rc::new(move |target| open_path(&ui_crumb, target)),
    );

    view.page.list.remove_all();
    if let Some(path) = path.as_deref()
        && view.asset_folders.borrow().contains(path)
    {
        view.page.list.append(&photos_row(ui, path));
    }
    let children = subfolders(&folders, path.as_deref());
    for child in &children {
        view.page.list.append(&folder_row(
            ui,
            child,
            subfolders(&folders, Some(child)).len(),
        ));
    }
    let empty = (
        "No Folders",
        "Folders appear once Immich has photos from them.",
    );
    show_empty_state(
        &view.page,
        (view.page.list.first_child().is_none()).then_some(empty),
    );
}

fn photos_row(ui: &Rc<LibraryWindowUi>, path: &str) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(format!("Photos in {}", leaf_name(path)))
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("image-x-generic-symbolic"));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let (ui, path) = (ui.clone(), path.to_string());
    row.connect_activated(move |_| open_photos(&ui, &path));
    row
}

fn folder_row(ui: &Rc<LibraryWindowUi>, path: &str, children: usize) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(leaf_name(path))
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
    let (ui, path) = (ui.clone(), path.to_string());
    if children == 0 {
        row.set_tooltip_text(Some("Show photos"));
        row.connect_activated(move |_| open_photos(&ui, &path));
    } else {
        row.set_subtitle(&match children {
            1 => "1 folder".to_string(),
            n => format!("{n} folders"),
        });
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        row.connect_activated(move |_| open_path(&ui, Some(path.clone())));
    }
    row
}

pub(super) fn sort_by_date(assets: &mut [LibraryAsset], order: Option<SortOrder>) {
    match order {
        Some(SortOrder::Asc) => assets.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
        Some(SortOrder::Desc) => assets.sort_by(|a, b| b.created_at.cmp(&a.created_at)),
        None => {}
    }
}

fn all_folders(paths: &[String]) -> BTreeSet<String> {
    let mut folders = BTreeSet::new();
    for path in paths {
        let mut current = Some(path.as_str());
        while let Some(folder) = current {
            if !folders.insert(folder.to_string()) {
                break;
            }
            current = parent_path(folder);
        }
    }
    folders
}

fn subfolders<'a>(folders: &'a BTreeSet<String>, parent: Option<&str>) -> Vec<&'a str> {
    folders
        .iter()
        .map(String::as_str)
        .filter(|folder| parent_path(folder) == parent)
        .collect()
}

fn start_path(folders: &BTreeSet<String>, asset_folders: &HashSet<String>) -> Option<String> {
    let mut current: Option<&str> = None;
    loop {
        if current.is_some_and(|path| asset_folders.contains(path)) {
            break;
        }
        match subfolders(folders, current).as_slice() {
            [only] => current = Some(only),
            _ => break,
        }
    }
    current.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn all_folders_adds_every_ancestor() {
        let folders = all_folders(&paths(&["/lib/admin/2024/01", "/lib/admin/2023"]));
        let listed: Vec<&str> = folders.iter().map(String::as_str).collect();
        assert_eq!(
            listed,
            [
                "/lib",
                "/lib/admin",
                "/lib/admin/2023",
                "/lib/admin/2024",
                "/lib/admin/2024/01"
            ]
        );
        assert_eq!(subfolders(&folders, None), ["/lib"]);
        assert_eq!(
            subfolders(&folders, Some("/lib/admin")),
            ["/lib/admin/2023", "/lib/admin/2024"]
        );
    }

    #[test]
    fn start_path_skips_single_child_chains() {
        let listed = paths(&["/lib/admin/2024/01", "/lib/admin/2023"]);
        let assets: HashSet<String> = listed.iter().cloned().collect();
        assert_eq!(
            start_path(&all_folders(&listed), &assets).as_deref(),
            Some("/lib/admin")
        );
    }

    #[test]
    fn start_path_stops_at_a_folder_with_photos() {
        let listed = paths(&["/lib", "/lib/admin/2024"]);
        let assets: HashSet<String> = listed.iter().cloned().collect();
        assert_eq!(
            start_path(&all_folders(&listed), &assets).as_deref(),
            Some("/lib")
        );
    }

    #[test]
    fn sort_by_date_follows_the_order() {
        let asset = |id: &str, at: &str| -> LibraryAsset {
            serde_json::from_value(serde_json::json!({
                "id": id,
                "originalFileName": "a.jpg",
                "originalMimeType": "image/jpeg",
                "fileCreatedAt": at,
                "type": "IMAGE"
            }))
            .unwrap()
        };
        let mut assets = vec![
            asset("old", "2020-01-01T00:00:00.000Z"),
            asset("new", "2024-01-01T00:00:00.000Z"),
        ];
        sort_by_date(&mut assets, Some(SortOrder::Desc));
        assert_eq!(assets[0].id, "new");
        sort_by_date(&mut assets, Some(SortOrder::Asc));
        assert_eq!(assets[0].id, "old");
    }
}
