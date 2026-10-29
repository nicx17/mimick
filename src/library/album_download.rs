//! Download a whole album as one zip, or as numbered parts when Immich splits it.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::api_client::DownloadArchive;

use super::LibraryWindowUi;
use super::context_menu::show_alert_dialog;
use super::dialogs::toast_with_action;
use super::download::{
    begin_download_session, ensure_download_target, finish_download_item,
    open_local_with_default_app, track_download_item, unique_path,
};

pub(super) fn archive_file_name(album_name: &str, index: usize, count: usize) -> String {
    let base = crate::sanitize::safe_filename(&album_name.replace('/', "-"))
        .unwrap_or_else(|| "Album".to_string());
    if count <= 1 {
        format!("{base}.zip")
    } else {
        format!("{base} (part {} of {count}).zip", index + 1)
    }
}

pub(super) fn download_album(ui: Rc<LibraryWindowUi>, album_id: String, album_name: String) {
    glib::MainContext::default().spawn_local(async move {
        let info = match ui.ctx.api_client.album_download_info(&album_id).await {
            Ok(info) => info,
            Err(err) => {
                let body = format!("Immich couldn't prepare the album: {err}");
                return show_alert_dialog(&ui, "Download Failed", &body);
            }
        };
        if info.archives.is_empty() {
            let message = format!("“{album_name}” has nothing to download");
            ui.toasts.add_toast(libadwaita::Toast::new(&message));
            return;
        }
        let Some(dir) = ensure_download_target(&ui).await else {
            return;
        };
        begin_download_session(&ui.ctx, format!("{album_name}.zip"));
        match save_archives(&ui, (&album_id, &album_name), &info.archives, &dir).await {
            Ok(saved) => report_saved(&ui, &saved, &dir),
            Err(err) => show_alert_dialog(&ui, "Download Failed", &err),
        }
    });
}

async fn save_archives(
    ui: &LibraryWindowUi,
    (album_id, album_name): (&str, &str),
    archives: &[DownloadArchive],
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut saved = Vec::with_capacity(archives.len());
    for (index, archive) in archives.iter().enumerate() {
        let name = archive_file_name(album_name, index, archives.len());
        let mut path = dir.join(&name);
        if path.exists() {
            path = unique_path(dir, &name);
        }
        let item_id = format!("zip:{album_id}:{index}");
        let progress =
            track_download_item(&ui.ctx, item_id.clone(), Some(name), Some(archive.size));
        let result = ui
            .ctx
            .api_client
            .download_archive_to_file(&archive.asset_ids, &path, Some(progress))
            .await;
        finish_download_item(&ui.ctx, &item_id);
        result?;
        saved.push(path);
    }
    Ok(saved)
}

fn report_saved(ui: &LibraryWindowUi, saved: &[PathBuf], dir: &Path) {
    let message = match saved {
        [one] => format!(
            "Saved {}",
            one.file_name().unwrap_or_default().to_string_lossy()
        ),
        many => format!("Saved {} zip files", many.len()),
    };
    let dir = dir.display().to_string();
    ui.toasts
        .add_toast(toast_with_action(&message, "Open Folder", move || {
            open_local_with_default_app(&dir)
        }));
}

#[cfg(test)]
mod tests {
    use super::archive_file_name;

    #[test]
    fn archive_file_name_single() {
        assert_eq!(archive_file_name("Trip", 0, 1), "Trip.zip");
    }

    #[test]
    fn archive_file_name_parts() {
        assert_eq!(archive_file_name("Trip", 1, 3), "Trip (part 2 of 3).zip");
    }

    #[test]
    fn archive_file_name_sanitizes() {
        let name = archive_file_name("a/b", 0, 1);
        assert!(!name.contains('/'));
        assert!(name.ends_with(".zip"));
        assert_eq!(archive_file_name("", 0, 1), "Album.zip");
    }
}
