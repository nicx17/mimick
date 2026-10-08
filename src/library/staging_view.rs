//! Staging view for files opened via "Open With" or the `--upload` CLI flag.
//!
//! Presents externally-provided file paths in a masonry grid with selection
//! checkboxes and Upload / Upload to Album actions. Reuses the existing
//! `MasonryCanvas`, `LibraryAssetModel`, `AssetObject`, `ThumbnailCache`,
//! and `spawn_enqueue` infrastructure.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use glib::clone;
use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::api_client::LibraryAlbum;
use crate::app_context::AppContext;
use crate::library::asset_object::AssetObject;
use crate::library::masonry::build_grid_view;
use crate::library::style;
use crate::library::upload_picker;
use crate::media_kinds;

/// Build and present the staging window populated with the given file paths.
///
/// When `auto_upload` is true (the `--upload` Desktop Action), an album
/// picker dialog is presented immediately after the window opens.
pub fn build_staging_window(
    app: &libadwaita::Application,
    ctx: Arc<AppContext>,
    files: Vec<PathBuf>,
    auto_upload: bool,
) {
    style::ensure_registered();
    crate::library::register_app_icons();

    let window = libadwaita::ApplicationWindow::builder()
        .application(app)
        .title("Mimick - Upload")
        .name("mimick-staging-window")
        .default_width(960)
        .default_height(640)
        .width_request(360)
        .height_request(400)
        .build();

    let (upload_btn, upload_album_btn, select_all_btn, grid, status_label) =
        build_staging_ui(&window, &ctx, &files);
    setup_narrow_breakpoint(&window, &grid);

    let file_count = grid.model.n_items();
    let files_rc = Rc::new(files);
    let selection = grid.selection.clone();

    connect_select_all(&select_all_btn, &selection, &status_label, file_count);
    connect_selection_tracking(&selection, &status_label);
    connect_upload_buttons(
        &upload_btn,
        &upload_album_btn,
        &ctx,
        &selection,
        &files_rc,
        &window,
    );

    // Accept additional file drops onto the staging window.
    connect_staging_drop_target(&window, &grid.model, &status_label);

    window.present();

    if auto_upload {
        let all_paths = files_rc.to_vec();
        selection.select_all();
        glib::idle_add_local_once(clone!(
            #[strong]
            window,
            #[strong]
            ctx,
            move || {
                show_album_picker(window, ctx, all_paths);
            }
        ));
    }
}

/// Build header, grid, toolbar, status bar and return the interactive widgets.
fn build_staging_ui(
    window: &libadwaita::ApplicationWindow,
    ctx: &Arc<AppContext>,
    files: &[PathBuf],
) -> (
    gtk::Button,
    gtk::Button,
    gtk::Button,
    crate::library::masonry::grid_view::GridViewParts,
    gtk::Label,
) {
    let (header, upload_btn, upload_album_btn, select_all_btn) = build_staging_header();
    let toolbar = libadwaita::ToolbarView::builder().build();
    toolbar.add_top_bar(&header);

    let select_toggle = gtk::ToggleButton::builder().active(true).build();
    let narrow = Rc::new(Cell::new(false));
    let grid = build_grid_view(ctx.clone(), select_toggle.clone(), narrow.clone());
    grid.canvas.set_select_mode(true);
    grid.model.reset_with_objects(build_staging_assets(files));

    let status_label = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .hexpand(true)
        .margin_start(12)
        .margin_end(12)
        .margin_top(4)
        .margin_bottom(4)
        .build();
    status_label.set_text(&status_text(0, grid.model.n_items()));

    toolbar.set_content(Some(&grid.scrolled));
    toolbar.add_bottom_bar(&status_label);
    window.set_content(Some(&toolbar));

    (
        upload_btn,
        upload_album_btn,
        select_all_btn,
        grid,
        status_label,
    )
}

/// Build the staging header bar with upload, album, and select-all buttons.
fn build_staging_header() -> (libadwaita::HeaderBar, gtk::Button, gtk::Button, gtk::Button) {
    let header = libadwaita::HeaderBar::builder()
        .show_start_title_buttons(true)
        .show_end_title_buttons(true)
        .build();
    let upload_btn = gtk::Button::builder()
        .label("Upload")
        .tooltip_text("Upload selected files to library")
        .css_classes(["suggested-action", "mimick-pressable"])
        .build();
    let album_btn = gtk::Button::builder()
        .label("Album")
        .tooltip_text("Upload selected files to an album")
        .css_classes(["mimick-pressable"])
        .build();
    let select_btn = gtk::Button::builder()
        .icon_name("edit-select-all-symbolic")
        .tooltip_text("Select all / Deselect all")
        .css_classes(["mimick-pressable"])
        .build();
    header.pack_start(&upload_btn);
    header.pack_start(&album_btn);
    header.pack_end(&select_btn);
    (header, upload_btn, album_btn, select_btn)
}

/// Install a 600sp breakpoint to toggle narrow grid layout.
fn setup_narrow_breakpoint(
    window: &libadwaita::ApplicationWindow,
    grid: &crate::library::masonry::grid_view::GridViewParts,
) {
    let bp = libadwaita::Breakpoint::new(
        libadwaita::BreakpointCondition::parse("max-width: 600sp")
            .expect("valid breakpoint condition"),
    );
    let c1 = grid.canvas.clone();
    bp.connect_apply(move |_| c1.set_narrow(true));
    let c2 = grid.canvas.clone();
    bp.connect_unapply(move |_| c2.set_narrow(false));
    window.add_breakpoint(bp);
}

fn connect_select_all(
    btn: &gtk::Button,
    selection: &gtk::MultiSelection,
    status_label: &gtk::Label,
    file_count: u32,
) {
    let sel = selection.clone();
    let lbl = status_label.clone();
    let n = file_count;
    let all_selected = Rc::new(Cell::new(false));
    btn.connect_clicked(move |btn| {
        if all_selected.get() {
            sel.unselect_all();
            btn.set_icon_name("edit-select-all-symbolic");
            btn.set_tooltip_text(Some("Select all"));
            all_selected.set(false);
            lbl.set_text(&status_text(0, n));
        } else {
            sel.select_all();
            btn.set_icon_name("edit-clear-all-symbolic");
            btn.set_tooltip_text(Some("Deselect all"));
            all_selected.set(true);
            lbl.set_text(&status_text(n, n));
        }
    });
}

fn connect_selection_tracking(selection: &gtk::MultiSelection, status_label: &gtk::Label) {
    let sel = selection.clone();
    let lbl = status_label.clone();
    sel.connect_selection_changed(move |sel, _, _| {
        let total = sel.n_items();
        let selected = count_selected(sel);
        lbl.set_text(&status_text(selected, total));
    });
}

/// Format the status bar text for selected/total counts.
fn status_text(selected: u32, total: u32) -> String {
    if selected == 0 {
        format!("{} file(s) staged", total)
    } else {
        format!("{} of {} selected", selected, total)
    }
}

/// Wire both upload buttons; they share identical clone setup, differing only in action.
fn connect_upload_buttons(
    upload_btn: &gtk::Button,
    album_btn: &gtk::Button,
    ctx: &Arc<AppContext>,
    selection: &gtk::MultiSelection,
    files_rc: &Rc<Vec<PathBuf>>,
    window: &libadwaita::ApplicationWindow,
) {
    let (sel1, files1, ctx1, win1) = (
        selection.clone(),
        files_rc.clone(),
        ctx.clone(),
        window.clone(),
    );
    upload_btn.connect_clicked(move |_| {
        let paths = selected_paths(&sel1, &files1);
        if paths.is_empty() {
            return;
        }
        let w = win1.clone();
        upload_picker::spawn_enqueue_with_callback(
            ctx1.clone(),
            None,
            paths,
            move |queued, skipped| show_upload_result(&w, queued, skipped),
        );
    });

    let (sel2, files2, ctx2, win2) = (
        selection.clone(),
        files_rc.clone(),
        ctx.clone(),
        window.clone(),
    );
    album_btn.connect_clicked(move |_| {
        let paths = selected_paths(&sel2, &files2);
        if paths.is_empty() {
            return;
        }
        show_album_picker(win2.clone(), ctx2.clone(), paths);
    });
}

/// Build `AssetObject` entries from local file paths for the staging grid.
fn build_staging_assets(files: &[PathBuf]) -> Vec<AssetObject> {
    files
        .iter()
        .filter(|path| media_kinds::is_supported_path(path))
        .filter_map(|path| {
            let filename = path.file_name()?.to_string_lossy().to_string();
            let mime = media_kinds::mime_for_path(path);
            let kind = media_kinds::asset_kind(mime);
            let asset_type = match kind {
                media_kinds::AssetKind::Image => "IMAGE",
                media_kinds::AssetKind::Video => "VIDEO",
            };
            let id = format!("local::{}", path.display());
            let mtime_str = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(systemtime_to_iso)
                .unwrap_or_default();

            Some(AssetObject::new_local(
                &id,
                &filename,
                mime,
                &mtime_str,
                asset_type,
                &path.to_string_lossy(),
            ))
        })
        .collect()
}

/// Format a `SystemTime` as an ISO 8601 string in the local timezone.
fn systemtime_to_iso(t: std::time::SystemTime) -> String {
    use chrono::{DateTime, Local};
    let dt: DateTime<Local> = t.into();
    dt.to_rfc3339()
}

/// Count the number of selected items in a `MultiSelection`.
fn count_selected(sel: &gtk::MultiSelection) -> u32 {
    let bs = sel.selection();
    let mut count = 0u32;
    let n = bs.size();
    for i in 0..n {
        if bs.nth(i as u32) != gtk::INVALID_LIST_POSITION {
            count += 1;
        }
    }
    // Fallback: iterate all items
    if count == 0 && n > 0 {
        count = n as u32;
    }
    count
}

/// Collect file paths for the selected items, or all items if none selected.
fn selected_paths(sel: &gtk::MultiSelection, files: &[PathBuf]) -> Vec<PathBuf> {
    let bs = sel.selection();
    let total = sel.n_items();
    let mut paths = Vec::new();

    // Collect indices of selected items.
    let mut selected_indices = Vec::new();
    let n_ranges = bs.size();
    if n_ranges > 0 {
        for i in 0..total {
            if sel.is_selected(i) {
                selected_indices.push(i as usize);
            }
        }
    }

    if selected_indices.is_empty() {
        // Nothing explicitly selected -- upload all.
        return files
            .iter()
            .filter(|p| media_kinds::is_supported_path(p))
            .cloned()
            .collect();
    }

    for idx in selected_indices {
        if let Some(item) = sel.item(idx as u32).and_downcast::<AssetObject>() {
            let local_path = item.property::<String>("local-path");
            if !local_path.is_empty() {
                paths.push(PathBuf::from(local_path));
            }
        }
    }
    paths
}

/// Present an album picker dialog and upload files to the chosen album.
pub(crate) fn show_album_picker(
    parent: libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    paths: Vec<PathBuf>,
) {
    let dialog = simple_alert("Upload to Album", "Loading albums...", "cancel", "Cancel");
    dialog.present(Some(&parent));

    let ctx_c = ctx.clone();
    let parent_c = parent.clone();
    glib::MainContext::default().spawn_local(async move {
        let albums = match ctx_c.api_client.fetch_library_albums().await {
            Ok(a) => a,
            Err(err) => {
                dialog.force_close();
                simple_alert("Failed to Load Albums", &err, "ok", "OK").present(Some(&parent_c));
                return;
            }
        };
        dialog.force_close();
        present_album_list(parent_c, ctx_c, albums, paths);
    });
}

/// Build and show the album selection list dialog.
fn present_album_list(
    parent: libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    albums: Vec<LibraryAlbum>,
    paths: Vec<PathBuf>,
) {
    let parent_for_result = parent.clone();
    let dialog = libadwaita::AlertDialog::builder()
        .heading("Select Album")
        .body("Choose an album for the upload:")
        .build();
    dialog.add_response("cancel", "Cancel");

    let (scroll, listbox) = build_album_listbox(&albums);
    dialog.set_extra_child(Some(&scroll));

    let albums_rc = Rc::new(albums);
    let paths_rc = Rc::new(paths);
    let ctx_c = ctx.clone();
    dialog.connect_response(None, move |_dialog, response| {
        if response == "cancel" {
            return;
        }
        handle_album_upload(&listbox, &albums_rc, &ctx_c, &paths_rc, &parent_for_result);
    });

    dialog.add_response("upload", "Upload");
    dialog.set_response_appearance("upload", libadwaita::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("upload"));
    dialog.present(Some(&parent));
}

/// Handle the album upload response: look up selected row and enqueue.
fn handle_album_upload(
    listbox: &gtk::ListBox,
    albums: &[LibraryAlbum],
    ctx: &Arc<AppContext>,
    paths: &Rc<Vec<PathBuf>>,
    parent: &libadwaita::ApplicationWindow,
) {
    let Some(row) = listbox.selected_row() else {
        return;
    };
    let idx = row.index() as usize;
    if let Some(album) = albums.get(idx) {
        let album_arg = Some((album.id.clone(), album.album_name.clone()));
        let parent_c = parent.clone();
        upload_picker::spawn_enqueue_with_callback(
            ctx.clone(),
            album_arg,
            paths.to_vec(),
            move |queued, skipped| show_upload_result(&parent_c, queued, skipped),
        );
    }
}

/// Build the scrollable album list for the picker dialog.
fn build_album_listbox(albums: &[LibraryAlbum]) -> (gtk::ScrolledWindow, gtk::ListBox) {
    let listbox = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .css_classes(["boxed-list"])
        .build();
    for album in albums {
        let row = libadwaita::ActionRow::builder()
            .title(&album.album_name)
            .subtitle(format!("{} assets", album.asset_count))
            .activatable(true)
            .build();
        listbox.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&listbox)
        .min_content_height(200)
        .max_content_height(400)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    (scroll, listbox)
}

/// Show a result dialog after upload enqueue completes.
fn show_upload_result(parent: &libadwaita::ApplicationWindow, queued: usize, skipped: usize) {
    let (heading, body) = if skipped == 0 {
        (
            "Upload Queued",
            format!(
                "{} file(s) queued for upload. Progress is visible in the queue inspector.",
                queued
            ),
        )
    } else if queued == 0 {
        (
            "Upload Failed",
            format!(
                "All {} file(s) failed to enqueue. Check the logs for details.",
                skipped
            ),
        )
    } else {
        (
            "Upload Partially Queued",
            format!(
                "{} file(s) queued, {} skipped (hash or path errors).",
                queued, skipped
            ),
        )
    };

    simple_alert(heading, &body, "ok", "OK").present(Some(parent));
}

/// Create a one-button `AlertDialog` (shared pattern used by multiple dialogs).
fn simple_alert(
    heading: &str,
    body: &str,
    response_id: &str,
    response_label: &str,
) -> libadwaita::AlertDialog {
    let dialog = libadwaita::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response(response_id, response_label);
    dialog
}

/// Attach a `DropTarget` to the staging window for appending additional files.
fn connect_staging_drop_target(
    window: &libadwaita::ApplicationWindow,
    model: &crate::library::asset_model::LibraryAssetModel,
    status_label: &gtk::Label,
) {
    use std::collections::HashSet;

    let drop_target = gtk::DropTarget::new(
        gtk::gdk::FileList::static_type(),
        gtk::gdk::DragAction::COPY,
    );

    let model = model.clone();
    let label = status_label.clone();
    drop_target.connect_drop(move |_target, value, _x, _y| {
        let file_list = match value.get::<gtk::gdk::FileList>() {
            Ok(fl) => fl,
            Err(_) => return false,
        };

        // Collect existing paths to deduplicate.
        let mut existing: HashSet<String> = HashSet::new();
        for i in 0..model.n_items() {
            if let Some(obj) = model.item(i).and_downcast::<AssetObject>() {
                let lp = obj.property::<String>("local-path");
                if !lp.is_empty() {
                    existing.insert(lp);
                }
            }
        }

        let new_paths: Vec<PathBuf> = file_list
            .files()
            .iter()
            .filter_map(gtk::gio::prelude::FileExt::path)
            .filter(|p| media_kinds::is_supported_path(p))
            .filter(|p| !existing.contains(&p.to_string_lossy().to_string()))
            .collect();

        if new_paths.is_empty() {
            return true;
        }

        let new_assets = build_staging_assets(&new_paths);
        model.append_objects(&new_assets);

        let total = model.n_items();
        label.set_text(&status_text(0, total));
        true
    });

    window.add_controller(drop_target);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_staging_assets_filters_unsupported() {
        let files = vec![
            PathBuf::from("/tmp/photo.jpg"),
            PathBuf::from("/tmp/document.pdf"),
            PathBuf::from("/tmp/video.mp4"),
            PathBuf::from("/tmp/readme.txt"),
        ];
        let assets = build_staging_assets(&files);
        // .jpg and .mp4 are supported; .pdf and .txt are not.
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].property::<String>("filename"), "photo.jpg");
        assert_eq!(assets[1].property::<String>("filename"), "video.mp4");
    }

    #[test]
    fn build_staging_assets_sets_asset_type() {
        let files = vec![
            PathBuf::from("/tmp/photo.png"),
            PathBuf::from("/tmp/clip.mkv"),
        ];
        let assets = build_staging_assets(&files);
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].property::<String>("asset-type"), "IMAGE");
        assert_eq!(assets[1].property::<String>("asset-type"), "VIDEO");
    }
}
