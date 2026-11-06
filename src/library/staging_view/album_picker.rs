//! Album selection dialog and upload result handling for staged files.

use crate::i18n::{fill, plural_n};
use gettextrs::{gettext, ngettext};
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
        gettext("Upload to Album"),
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
            &gettext("Upload Queued"),
            fill(
                ngettext(
                    "{n} file queued for upload. Progress is visible in the queue inspector.",
                    "{n} files queued for upload. Progress is visible in the queue inspector.",
                    plural_n(queued),
                ),
                &[("n", &queued.to_string())],
            ),
        )
    } else if queued == 0 {
        (
            &gettext("Upload Failed"),
            fill(
                ngettext(
                    "{n} file failed to enqueue. Check the logs for details.",
                    "All {n} files failed to enqueue. Check the logs for details.",
                    plural_n(skipped),
                ),
                &[("n", &skipped.to_string())],
            ),
        )
    } else {
        (
            &gettext("Upload Partially Queued"),
            fill(
                ngettext(
                    "{n} file queued, {skipped} skipped (hash or path errors).",
                    "{n} files queued, {skipped} skipped (hash or path errors).",
                    plural_n(queued),
                ),
                &[
                    ("n", &queued.to_string()),
                    ("skipped", &skipped.to_string()),
                ],
            ),
        )
    };

    show_alert_on(parent, heading, &body);
}
