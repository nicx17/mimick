use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAsset;
use crate::library::local_source::{
    LocalAsset, enumerate_local, enumerate_local_for_entry, filter_by_filename,
};
use crate::library::state::{LibraryLoadState, LibrarySource};
use crate::library::{
    LOCAL_ID_PREFIX, LibraryWindowUi, album_assets, navigation, setup_album_drop_target, sidebar,
    source_header,
};
use crate::state_manager::TransferDirection;

pub(super) fn build_album_sidebar_row(
    ui: &Rc<LibraryWindowUi>,
    album: &crate::api_client::LibraryAlbum,
) -> gtk::ListBoxRow {
    let subtitle = format!("{} asset(s)", album.asset_count);
    let row: gtk::ListBoxRow = libadwaita::ActionRow::builder()
        .title(&album.album_name)
        .subtitle(&subtitle)
        .title_lines(1)
        .subtitle_lines(1)
        .tooltip_text(format!("{}:{}", album.id, album.album_name))
        .build()
        .upcast();

    // Per-row drop target: dragging files onto an album row uploads
    // them directly into that album.
    setup_album_drop_target(ui, &row, album.id.clone(), album.album_name.clone());

    row
}

pub(super) fn reload_sidebar(ui: &Rc<LibraryWindowUi>) {
    album_assets::sync_bulk_remove(ui);
    let albums = ui.ctx.library_state.lock().albums.clone();
    sidebar::set_recent_albums(&ui.sidebar, &albums);
    if sidebar::album_list_changed(&ui.sidebar, &albums) {
        let had_focus = ui.sidebar.albums_list.focus_child().is_some();
        ui.interaction.sidebar_suppressed.set(true);
        while let Some(row) = ui.sidebar.albums_list.first_child() {
            ui.sidebar.albums_list.remove(&row);
        }
        for album in &albums {
            let row = build_album_sidebar_row(ui, album);
            ui.sidebar.albums_list.append(&row);
        }
        ui.interaction.sidebar_suppressed.set(false);
        navigation::sync_sidebar(ui);
        if had_focus {
            match ui.sidebar.albums_list.selected_row() {
                Some(row) => row.grab_focus(),
                None => ui.sidebar.fixed_lists[0].grab_focus(),
            };
        }
        return;
    }
    navigation::sync_sidebar(ui);
}

/// Sync the visibility of the grid, loading, empty, and error page widgets.
///
/// The lock on `library_state` is released **before** calling
/// `set_visible_child_name` because that GTK call triggers widget
/// realization, factory binds, and signal handlers that may need to
/// re-acquire the same lock.  Holding it across the call caused a
/// parking_lot deadlock on first library open.
pub(super) fn sync_content_state(ui: &LibraryWindowUi) {
    if navigation::on_page(ui) {
        return;
    }
    let (status, error_msg, single_page_count) = {
        let state = ui.ctx.library_state.lock();
        let single_page = matches!(
            state.source,
            LibrarySource::Folder { .. }
                | LibrarySource::LocalAll
                | LibrarySource::LocalSearch { .. }
        );
        let count = (single_page && !state.has_more).then_some(state.assets.len());
        match &state.load_state {
            LibraryLoadState::Idle | LibraryLoadState::Loading => ("loading", None, None),
            LibraryLoadState::Loaded => ("none", None, count),
            LibraryLoadState::Empty => ("empty", None, count),
            LibraryLoadState::Error(msg) => ("error", Some(msg.clone()), None),
        }
    };
    source_header::set_status(ui, status, error_msg.as_deref());
    if let Some(count) = single_page_count {
        source_header::set_loaded_count(ui, count);
    }
    ui.content_stack.set_visible_child_name("grid");
}

/// Update the status sidebar rows with the current server route and statistics.
pub(super) fn update_footer(ui: &LibraryWindowUi, route: Option<String>) {
    let (stats_text, about_text) = {
        let state = ui.ctx.library_state.lock();
        let stats = state
            .status
            .stats
            .as_ref()
            .map(|stats| format!("{} photos, {} videos", stats.images, stats.videos))
            .unwrap_or_else(|| "Statistics unavailable".to_string());
        let about = state
            .status
            .about
            .as_ref()
            .map(|about| format!("Immich {}", about.version))
            .unwrap_or_else(|| "Version unavailable".to_string());
        (stats, about)
    };
    let route_subtitle = route
        .as_deref()
        .map(|route| match route {
            "LAN" => "Connected through LAN",
            "WAN" => "Connected through WAN",
            _ => "Connected through configured server",
        })
        .unwrap_or("Offline");
    ui.sidebar.connection_row.set_subtitle(route_subtitle);
    ui.sidebar
        .server_row
        .set_subtitle(&format!("{stats_text} | {about_text}"));
}

/// Synchronize the ongoing upload/download progress indicator bar and rate information text.
pub(super) fn update_transfer_ui(ui: &LibraryWindowUi) {
    let transfer = {
        let mut state = ui.ctx.state.lock();
        if state.transfer.active
            && state.transfer.active_uploads == 0
            && state.transfer.active_downloads == 0
        {
            // Guard against sessions that were opened but never queued.
            state.transfer.reset_runtime();
        }
        state.transfer.clone()
    };
    ui.transfer.indicator.set_visible(transfer.active);
    if !transfer.active {
        return;
    }
    let icon_name = match transfer.direction {
        TransferDirection::Upload => "mimick-upload-symbolic",
        TransferDirection::Download => "mimick-download-symbolic",
    };
    ui.transfer.icon.set_icon_name(Some(icon_name));
    let detail = transfer
        .active_item_label
        .as_deref()
        .unwrap_or(match transfer.direction {
            TransferDirection::Upload => "queued asset",
            TransferDirection::Download => "selected asset",
        });
    ui.transfer.indicator.set_tooltip_text(Some(detail));
    match transfer.total_bytes {
        Some(total) if total > 0 => {
            let fraction = (transfer.current_bytes as f64 / total as f64).clamp(0.0, 1.0);
            ui.transfer.progress.set_fraction(fraction);
        }
        _ => ui.transfer.progress.pulse(),
    }
}

/// Construct a styled placeholder view containing an icon, header title, and description text.
/// Centered Mimick-icon spinner used while library data is fetching. Shares
/// the `mimick-loader-icon` animation with the lightbox image-load spinner.
fn build_empty_state_container() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .vexpand(true)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Center)
        .css_classes(vec!["mimick-empty".to_string()])
        .build()
}

pub(super) fn build_loading_view() -> gtk::Box {
    let container = build_empty_state_container();
    let icon = gtk::Image::builder()
        .icon_name("dev.nicx.mimick")
        .pixel_size(72)
        .css_classes(["mimick-loader-icon"])
        .build();
    let title = gtk::Label::builder()
        .label("Loading…")
        .css_classes(vec!["mimick-empty-title".to_string()])
        .build();
    let subtitle = gtk::Label::builder()
        .label("Fetching library data from the Immich server")
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .justify(gtk::Justification::Center)
        .max_width_chars(28)
        .css_classes(vec!["mimick-empty-subtitle".to_string()])
        .build();
    container.append(&icon);
    container.append(&title);
    container.append(&subtitle);
    container
}

pub(super) fn build_status_view(icon_name: &str, title: &str, subtitle: &str) -> gtk::Box {
    let container = build_empty_state_container();
    let icon = gtk::Image::builder()
        .icon_name(icon_name)
        .pixel_size(64)
        .build();
    icon.add_css_class("dim-label");
    let title_label = gtk::Label::builder()
        .label(title)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .justify(gtk::Justification::Center)
        .max_width_chars(28)
        .css_classes(vec!["mimick-empty-title".to_string()])
        .build();
    let subtitle_label = gtk::Label::builder()
        .label(subtitle)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .justify(gtk::Justification::Center)
        .max_width_chars(28)
        .css_classes(vec!["mimick-empty-subtitle".to_string()])
        .build();
    container.append(&icon);
    container.append(&title_label);
    container.append(&subtitle_label);
    container
}

/// Convert a Base64-encoded Immich checksum string to its standard hexadecimal representation.
pub(super) fn immich_checksum_to_hex(b64: &str) -> Option<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    Some(bytes.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Build a synthetic `LibraryAsset` from a physical local folder file.
pub(super) fn local_to_library_asset(local: LocalAsset) -> LibraryAsset {
    LibraryAsset {
        id: format!("{}{}", LOCAL_ID_PREFIX, local.path.display()),
        filename: local.filename,
        mime_type: local.mime,
        created_at: local.created_at,
        asset_type: local.asset_type.to_string(),
        thumbhash: None,
        width: None,
        height: None,
        checksum: None,
        exif_info: None,
        is_favorite: false,
        visibility: String::new(),
        is_edited: false,
        owner_id: String::new(),
        local_date_time: None,
    }
}

fn merge_remote_and_local(
    mut remote: Vec<LibraryAsset>,
    locals: Vec<LocalAsset>,
    ui: &LibraryWindowUi,
    has_more: bool,
) -> (Vec<LibraryAsset>, bool) {
    let synced_paths: std::collections::HashSet<String> = remote
        .iter()
        .filter_map(|asset| asset.checksum.as_deref())
        .filter_map(immich_checksum_to_hex)
        .filter_map(|hex| ui.ctx.sync_index.local_path_for_checksum(&hex))
        .collect();
    let mut local_rows: Vec<LibraryAsset> = locals
        .into_iter()
        .filter(|local| !synced_paths.contains(&local.path.display().to_string()))
        .map(local_to_library_asset)
        .collect();
    local_rows.append(&mut remote);
    (local_rows, has_more)
}

/// Merge a page of remote API assets with local files that haven't been synchronized yet.
pub(super) async fn merge_unified_page(
    remote: Result<(Vec<LibraryAsset>, bool), String>,
    page: u32,
    ui: &Rc<LibraryWindowUi>,
    query: Option<&str>,
) -> Result<(Vec<LibraryAsset>, bool), String> {
    let (remote, has_more) = remote?;
    if page > 1 {
        return Ok((remote, has_more));
    }

    let mut locals = enumerate_local(ui.ctx.clone()).await;
    if let Some(q) = query {
        locals = filter_by_filename(locals, q);
    }

    Ok(merge_remote_and_local(remote, locals, ui, has_more))
}

/// Return the path of the watch entry linked to `album_name`, if any.
/// Find the local directory watch path mapped to the specified album.
pub(super) fn linked_entry_path_for_album(
    ui: &Rc<LibraryWindowUi>,
    album_name: &str,
) -> Option<String> {
    let entries = ui.ctx.live_watch_paths.lock().clone();
    crate::config::watch_entry_for_album(album_name, &entries).map(|e| e.path().to_string())
}

/// Album-scoped variant of `merge_unified_page`: takes the album's asset
/// page from the remote API and overlays sync state from the album's
/// linked local folder only — never from siblings.
/// Merge a page of remote album assets with local folder files linked to the album.
pub(super) async fn merge_album_unified_page(
    remote: Result<(Vec<LibraryAsset>, bool), String>,
    page: u32,
    ui: &Rc<LibraryWindowUi>,
    album_name: &str,
) -> Result<(Vec<LibraryAsset>, bool), String> {
    let (remote, has_more) = remote?;
    if page > 1 {
        return Ok((remote, has_more));
    }

    let locals = match linked_entry_path_for_album(ui, album_name) {
        Some(path) => enumerate_local_for_entry(ui.ctx.clone(), path).await,
        None => Vec::new(),
    };

    Ok(merge_remote_and_local(remote, locals, ui, has_more))
}

/// Helper to show a permissions error dialog for library views.
pub(super) fn show_library_permission_error(window: &libadwaita::ApplicationWindow) {
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Missing API Permissions")
        .body("Your API key is missing permissions required for the Library view. Please ensure the key has 'asset.read', 'asset.view', 'asset.download', and 'person.read' enabled.")
        .build();
    dialog.add_response("close", "Close");
    dialog.present(Some(window));
}
