//! Manual upload entry point.
//!
//! Opens a multi-file picker, hashes each selection on a blocking worker, and
//! enqueues a `FileTask` on the shared `QueueManager`. The Upload header-bar
//! button in the library window invokes [`pick_and_upload`]; the same flow is
//! used for both library and album scopes — the caller resolves the album.

use std::path::PathBuf;
use std::sync::Arc;

use gtk::gio;
use gtk::prelude::*;

use crate::app_context::AppContext;
use crate::monitor::compute_sha1_chunked;
use crate::queue_manager::FileTask;

/// Open the OS file picker and enqueue every chosen file for upload.
///
/// `album` carries the target album id and display name when the caller is in
/// a selected-album view; otherwise pass `None` and files go to the library.
pub fn pick_and_upload(
    parent: &libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    album: Option<(String, String)>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(if album.is_some() {
            "Upload to album"
        } else {
            "Upload to library"
        })
        .modal(true)
        .build();

    let parent = parent.clone();
    dialog.open_multiple(Some(&parent), gio::Cancellable::NONE, move |result| {
        let files = match result {
            Ok(files) => files,
            Err(err) => {
                if !err.matches(gtk::DialogError::Dismissed) {
                    log::warn!("Upload picker failed: {}", err);
                }
                return;
            }
        };
        let paths: Vec<PathBuf> = (0..files.n_items())
            .filter_map(|i| files.item(i))
            .filter_map(|obj| obj.downcast::<gio::File>().ok())
            .filter_map(|file| file.path())
            .collect();
        if paths.is_empty() {
            return;
        }
        spawn_enqueue(ctx.clone(), album.clone(), paths);
    });
}

pub(crate) fn spawn_enqueue(
    ctx: Arc<AppContext>,
    album: Option<(String, String)>,
    paths: Vec<PathBuf>,
) {
    spawn_enqueue_with_callback(ctx, album, paths, |_, _| {});
}

/// Enqueue files for upload and invoke `on_complete(queued, skipped)` on the
/// main context when all files have been hashed and enqueued.
pub(crate) fn spawn_enqueue_with_callback<F>(
    ctx: Arc<AppContext>,
    album: Option<(String, String)>,
    paths: Vec<PathBuf>,
    on_complete: F,
) where
    F: FnOnce(usize, usize) + 'static,
{
    glib::MainContext::default().spawn_local(async move {
        let total = paths.len();
        let mut queued = 0usize;
        for path in paths {
            if let Some(task) = build_file_task(&ctx, &album, &path).await
                && ctx.queue_manager.add_to_queue(task).await
            {
                queued += 1;
            }
        }
        let skipped = total - queued;
        log::info!(
            "Manual upload: queued {} file(s), skipped {}",
            queued,
            skipped
        );
        on_complete(queued, skipped);
    });
}

/// Build a `FileTask` for a single path, computing checksum and resolving sidecar.
async fn build_file_task(
    ctx: &AppContext,
    album: &Option<(String, String)>,
    path: &std::path::Path,
) -> Option<FileTask> {
    let path_str = path.to_str().map(str::to_owned)?;
    let watch_path = path
        .parent()
        .and_then(std::path::Path::to_str)
        .map(str::to_owned)
        .unwrap_or_default();
    let hash_target = path_str.clone();
    let checksum =
        match tokio::task::spawn_blocking(move || compute_sha1_chunked(&hash_target)).await {
            Ok(Ok(c)) => c,
            Ok(Err(err)) => {
                log::warn!("Could not checksum upload '{}': {}", path_str, err);
                return None;
            }
            Err(err) => {
                log::warn!("Checksum task failed for '{}': {}", path_str, err);
                return None;
            }
        };
    let sidecar_path = if ctx.config.read().data.upload_xmp_sidecars {
        crate::sidecar::find_sidecar(path).map(|p| p.to_string_lossy().into_owned())
    } else {
        None
    };
    Some(FileTask {
        path: path_str,
        watch_path,
        checksum,
        album_id: album.as_ref().map(|(id, _)| id.clone()),
        album_name: album.as_ref().map(|(_, name)| name.clone()),
        reassociate_only: false,
        skip_album: album.is_none(),
        sidecar_path,
    })
}
