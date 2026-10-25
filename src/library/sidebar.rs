//! Sidebar navigation panel for the library window.
//!
//! Builds the split-pane sidebar containing album rows, fixed navigation
//! entries (All Assets, Explore, Local, Timeline), and the folder-scoped
//! album list. Handles selection changes to drive the main grid view source.

use std::cell::RefCell;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;

const RECENT_ALBUM_COUNT: usize = 3;
const RECENT_ALBUM_PREFIX: &str = "recent-album:";

/// Contains references to the side navigation panel widgets and lists.
pub struct SidebarParts {
    pub root: gtk::Box,
    pub connection_row: libadwaita::ActionRow,
    pub server_row: libadwaita::ActionRow,
    pub fixed_lists: Vec<gtk::ListBox>,
    pub albums_list: gtk::ListBox,
    recent_expander: gtk::ToggleButton,
    recent_rows: Vec<libadwaita::ActionRow>,
    album_signature: RefCell<Vec<(String, String, u32)>>,
}

const GROUPS: [(&str, &[&str]); 3] = [
    (
        "Library",
        &[
            "photos",
            "explore",
            "map",
            "people",
            "favorites",
            "archive",
            "trash",
        ],
    ),
    (
        "Albums & Sharing",
        &["albums", "shared-albums", "shared-links", "sharing"],
    ),
    ("Browse", &["search", "tags", "folders"]),
];

/// Fixed sidebar destinations: title, subtitle, icon, and the key the sidebar handler matches.
const FIXED_ROWS: [(&str, &str, &str, &str); 14] = [
    (
        "Photos",
        "Timeline of every photo and video",
        "image-x-generic-symbolic",
        "photos",
    ),
    (
        "Search",
        "Find photos, videos, and text",
        "system-search-symbolic",
        "search",
    ),
    (
        "Explore",
        "People, places, and things",
        "view-grid-symbolic",
        "explore",
    ),
    (
        "Map",
        "Photos by where they were taken",
        "mark-location-symbolic",
        "map",
    ),
    (
        "People",
        "Faces Immich has recognized",
        "avatar-default-symbolic",
        "people",
    ),
    (
        "Albums",
        "Recent, owned, and shared",
        "folder-pictures-symbolic",
        "albums",
    ),
    (
        "Shared Albums",
        "Shared with you and by you",
        "folder-publicshare-symbolic",
        "shared-albums",
    ),
    (
        "Shared Links",
        "Public links you've created",
        "emblem-shared-symbolic",
        "shared-links",
    ),
    (
        "Sharing",
        "Partners who share libraries",
        "system-users-symbolic",
        "sharing",
    ),
    (
        "Favorites",
        "Photos and videos you've marked",
        "mimick-heart-filled-symbolic",
        "favorites",
    ),
    (
        "Tags",
        "Browse and organize tags",
        "mimick-tag-symbolic",
        "tags",
    ),
    (
        "Folders",
        "Browse by folder on the server",
        "folder-symbolic",
        "folders",
    ),
    (
        "Archive",
        "Hidden from Photos",
        "mimick-archive-symbolic",
        "archive",
    ),
    (
        "Trash",
        "Restore or permanently delete",
        "user-trash-symbolic",
        "trash",
    ),
];

/// Connection and server rows at the bottom of the sidebar, in their own list.
fn build_footer() -> (gtk::ListBox, libadwaita::ActionRow, libadwaita::ActionRow) {
    let connection_row = footer_row("Connection", "Offline");
    let server_row = footer_row("Server", "Statistics unavailable");
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .build();
    list.append(&connection_row);
    list.append(&server_row);
    (list, connection_row, server_row)
}

/// Single-line activatable row for the connection/server footer.
fn footer_row(title: &str, subtitle: &str) -> libadwaita::ActionRow {
    libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .title_lines(1)
        .subtitle_lines(1)
        .activatable(true)
        .build()
}

/// Single-selection boxed list for the fixed entries and the album rows.
fn selectable_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(vec!["boxed-list".to_string()])
        .build()
}

/// Construct the hierarchical side navigation sidebar panel.
pub fn build_sidebar() -> SidebarParts {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(200)
        .build();

    let (connection_list, connection_row, server_row) = build_footer();
    let recent_rows: Vec<libadwaita::ActionRow> = (0..RECENT_ALBUM_COUNT)
        .map(|_| recent_album_row())
        .collect();
    let recent_expander = build_recent_expander(recent_rows.clone());
    let (groups, fixed_lists) = build_groups(&recent_expander, &recent_rows);
    let (albums_expander, albums_list) = build_all_albums();
    root.append(&scrolling_lists(&groups, &albums_expander));
    root.append(&connection_list);

    SidebarParts {
        root,
        connection_row,
        server_row,
        fixed_lists,
        albums_list,
        recent_expander,
        recent_rows,
        album_signature: RefCell::default(),
    }
}

// The fixed rows and albums scroll together so the sidebar fits short (720 px) screens;
// only the connection footer stays pinned.
fn scrolling_lists(groups: &gtk::Box, albums: &gtk::Expander) -> gtk::ScrolledWindow {
    let lists = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    lists.append(groups);
    lists.append(albums);
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&lists)
        .build()
}

fn build_groups(
    recent_expander: &gtk::ToggleButton,
    recent_rows: &[libadwaita::ActionRow],
) -> (gtk::Box, Vec<gtk::ListBox>) {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let mut lists = Vec::new();
    for (heading, keys) in GROUPS {
        container.append(&section_heading(heading));
        let list = selectable_list();
        for &(title, subtitle, icon, key) in FIXED_ROWS.iter().filter(|row| keys.contains(&row.3)) {
            let row = action_row(title, subtitle, icon, key);
            list.append(&row);
            if key == "albums" {
                row.add_suffix(recent_expander);
                for recent in recent_rows {
                    list.append(recent);
                }
            }
        }
        container.append(&list);
        lists.push(list);
    }
    (container, lists)
}

fn section_heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .margin_top(6)
        .css_classes(vec!["heading".to_string(), "dim-label".to_string()])
        .build()
}

pub fn select_fixed(parts: &SidebarParts, key: &str) {
    let row = parts.fixed_lists.iter().find_map(|list| {
        let mut child = list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Ok(row) = widget.downcast::<gtk::ListBoxRow>()
                && row.tooltip_text().as_deref() == Some(key)
            {
                return Some(row);
            }
        }
        None
    });
    match row {
        Some(row) => select_fixed_row(parts, &row),
        None => unselect_fixed(parts),
    }
}

pub fn select_fixed_row(parts: &SidebarParts, row: &gtk::ListBoxRow) {
    for list in &parts.fixed_lists {
        if row.parent().as_ref() == Some(list.upcast_ref()) {
            list.select_row(Some(row));
        } else {
            list.unselect_all();
        }
    }
}

pub fn set_row_visible(parts: &SidebarParts, key: &str, visible: bool) {
    for list in &parts.fixed_lists {
        let mut child = list.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if widget.tooltip_text().as_deref() == Some(key) {
                widget.set_visible(visible);
            }
        }
    }
}

pub fn unselect_fixed(parts: &SidebarParts) {
    for list in &parts.fixed_lists {
        list.unselect_all();
    }
}

fn build_recent_expander(rows: Vec<libadwaita::ActionRow>) -> gtk::ToggleButton {
    let expander = gtk::ToggleButton::builder()
        .icon_name("pan-end-symbolic")
        .tooltip_text("Show recent albums")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .visible(false)
        .build();
    expander.connect_toggled(move |button| {
        let expanded = button.is_active();
        button.set_icon_name(if expanded {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        });
        button.set_tooltip_text(Some(if expanded {
            "Hide recent albums"
        } else {
            "Show recent albums"
        }));
        for row in &rows {
            row.set_visible(expanded && recent_album_key(row.upcast_ref()).is_some());
        }
    });
    expander
}

fn build_all_albums() -> (gtk::Expander, gtk::ListBox) {
    let header = gtk::Label::builder()
        .label("All Albums")
        .xalign(0.0)
        .css_classes(vec!["heading".to_string(), "dim-label".to_string()])
        .build();
    let list = selectable_list();
    list.set_margin_top(12);
    let expander = gtk::Expander::builder()
        .label_widget(&header)
        .expanded(true)
        .margin_top(6)
        .child(&list)
        .build();
    (expander, list)
}

pub fn album_list_changed(parts: &SidebarParts, albums: &[LibraryAlbum]) -> bool {
    let signature: Vec<(String, String, u32)> = albums
        .iter()
        .map(|album| {
            (
                album.id.clone(),
                album.album_name.clone(),
                album.asset_count,
            )
        })
        .collect();
    let mut previous = parts.album_signature.borrow_mut();
    if *previous == signature {
        return false;
    }
    *previous = signature;
    true
}

pub fn set_recent_albums(parts: &SidebarParts, albums: &[LibraryAlbum]) {
    let recent = recent_albums(albums);
    let expanded = parts.recent_expander.is_active();
    for (index, row) in parts.recent_rows.iter().enumerate() {
        match recent.get(index) {
            Some(album) => {
                row.set_title(&album.album_name);
                row.set_widget_name(&format!(
                    "{RECENT_ALBUM_PREFIX}{}:{}",
                    album.id, album.album_name
                ));
                row.set_visible(expanded);
            }
            None => {
                row.set_widget_name("");
                row.set_visible(false);
            }
        }
    }
    parts.recent_expander.set_visible(!recent.is_empty());
}

pub fn recent_row_for_album(parts: &SidebarParts, album_id: &str) -> Option<gtk::ListBoxRow> {
    parts
        .recent_rows
        .iter()
        .find(|row| {
            row.is_visible()
                && recent_album_key(row.upcast_ref()).is_some_and(|(id, _)| id == album_id)
        })
        .map(|row| row.clone().upcast())
}

pub fn recent_album_key(row: &gtk::ListBoxRow) -> Option<(String, String)> {
    let name = row.widget_name();
    let (id, album_name) = name.strip_prefix(RECENT_ALBUM_PREFIX)?.split_once(':')?;
    Some((id.to_string(), album_name.to_string()))
}

fn recent_albums(albums: &[LibraryAlbum]) -> Vec<&LibraryAlbum> {
    let mut recent: Vec<&LibraryAlbum> = albums.iter().collect();
    recent.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    recent.truncate(RECENT_ALBUM_COUNT);
    recent
}

fn recent_album_row() -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .use_markup(false)
        .title_lines(1)
        .activatable(true)
        .visible(false)
        .build();
    let icon = gtk::Image::builder()
        .icon_name("folder-pictures-symbolic")
        .margin_start(16)
        .css_classes(["dim-label"])
        .build();
    row.add_prefix(&icon);
    row
}

/// Construct a navigation list row decorated with an icon and titles.
fn action_row(title: &str, subtitle: &str, icon_name: &str, key: &str) -> libadwaita::ActionRow {
    let row = libadwaita::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .title_lines(1)
        .subtitle_lines(1)
        .tooltip_text(key)
        .activatable(true)
        .build();
    let icon = gtk::Image::from_icon_name(icon_name);
    row.add_prefix(&icon);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: &str, updated_at: &str) -> LibraryAlbum {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "albumName": format!("Album {id}"),
            "assetCount": 1,
            "albumThumbnailAssetId": null,
            "createdAt": "2024-01-01T00:00:00.000Z",
            "updatedAt": updated_at
        }))
        .unwrap()
    }

    #[test]
    fn recent_albums_are_the_latest_updated_first() {
        let albums = vec![
            album("a", "2024-01-01T00:00:00.000Z"),
            album("b", "2024-03-01T00:00:00.000Z"),
            album("c", "2024-02-01T00:00:00.000Z"),
            album("d", "2024-04-01T00:00:00.000Z"),
        ];
        let ids: Vec<&str> = recent_albums(&albums)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(ids, ["d", "b", "c"]);
    }

    #[test]
    fn every_row_sits_in_exactly_one_group() {
        for (_, _, _, key) in FIXED_ROWS {
            let groups = GROUPS
                .iter()
                .filter(|(_, keys)| keys.contains(&key))
                .count();
            assert_eq!(groups, 1, "{key}");
        }
        let grouped: usize = GROUPS.iter().map(|(_, keys)| keys.len()).sum();
        assert_eq!(grouped, FIXED_ROWS.len());
    }
}
