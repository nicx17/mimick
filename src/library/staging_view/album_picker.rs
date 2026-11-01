//! Album selection dialog and upload result handling for staged files.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;

use crate::app_context::AppContext;
use crate::library::album_picker;
use crate::library::context_menu::show_alert_on;
use crate::library::upload_picker;

/// Present the shared album picker and upload files to the chosen album.
pub(crate) fn show_album_picker(
    parent: libadwaita::ApplicationWindow,
    ctx: Arc<AppContext>,
    paths: Vec<PathBuf>,
) {
    let count = paths.len();
    let ctx_upload = ctx.clone();
    let parent_result = parent.clone();
    album_picker::choose(
        parent.upcast_ref(),
        ctx,
        "Upload to Album",
        count,
        Rc::new(move |id, name| {
            let parent = parent_result.clone();
            upload_picker::spawn_enqueue_with_callback(
                ctx_upload.clone(),
                Some((id, name)),
                paths.clone(),
                move |queued, skipped| show_upload_result(&parent, queued, skipped),
            );
        }),
    );
}

/// Show a result dialog after upload enqueue completes.
pub(super) fn show_upload_result(
    parent: &libadwaita::ApplicationWindow,
    queued: usize,
    skipped: usize,
) {
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

    show_alert_on(parent, heading, &body);
}
