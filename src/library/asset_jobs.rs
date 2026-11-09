//! "Refresh Faces", "Refresh Metadata", "Refresh Thumbnails", and "Refresh Video" in the photo
//! menu and the selection bar's menu: Immich redoes that work in the background.

use crate::i18n::fill;
use gettextrs::gettext;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;

use super::LibraryWindowUi;
use super::asset_object::AssetObject;
use super::context_menu::{Menu, show_error_toast};
use crate::api_client::AssetJob;

/// Immich usually has a new thumbnail by then; the cached one is dropped so the next view fetches it.
const THUMBNAIL_REFRESH_DELAY: Duration = Duration::from_secs(20);

/// What each job applies to, its menu icon and label, and the toast once it's queued.
struct JobEntry {
    job: AssetJob,
    images: bool,
    videos: bool,
    icon: &'static str,
    label: fn() -> String,
    queued: fn() -> String,
}

const JOBS: [JobEntry; 4] = [
    JobEntry {
        job: AssetJob::RefreshFaces,
        images: true,
        videos: false,
        icon: "avatar-default-symbolic",
        label: || gettext("Refresh Faces"),
        queued: || gettext("Immich is looking for faces again"),
    },
    JobEntry {
        job: AssetJob::RefreshMetadata,
        images: true,
        videos: true,
        icon: "view-refresh-symbolic",
        label: || gettext("Refresh Metadata"),
        queued: || gettext("Immich is reading the metadata again"),
    },
    JobEntry {
        job: AssetJob::RegenerateThumbnail,
        images: true,
        videos: true,
        icon: "image-x-generic-symbolic",
        label: || gettext("Refresh Thumbnails"),
        queued: || gettext("Immich is making new thumbnails"),
    },
    JobEntry {
        job: AssetJob::TranscodeVideo,
        images: false,
        videos: true,
        icon: "video-x-generic-symbolic",
        label: || gettext("Refresh Video"),
        queued: || gettext("Immich is converting the video again"),
    },
];

/// The refresh entries that fit `item`; Immich runs them only on the owner's photos.
pub(super) fn append_refresh_buttons(menu: &Menu, ui: &Rc<LibraryWindowUi>, item: &AssetObject) {
    let is_video = item
        .property::<String>("asset-type")
        .eq_ignore_ascii_case("VIDEO");
    let people = ui.features.get().people;
    for entry in &JOBS {
        let fits = if is_video { entry.videos } else { entry.images };
        if !fits || (entry.job == AssetJob::RefreshFaces && !people) {
            continue;
        }
        let (ui, id, job, queued) = (ui.clone(), item.id(), entry.job, entry.queued);
        menu.add_with_icon(entry.icon, &(entry.label)(), false, move || {
            run(&ui, job, vec![id.clone()], queued)
        });
    }
}

/// One button per job for the selection bar's menu, which clicks them; they're never shown.
pub(super) fn bulk_buttons() -> Vec<(AssetJob, gtk::Button)> {
    JOBS.iter()
        .map(|entry| {
            let button = gtk::Button::builder()
                .icon_name(entry.icon)
                .tooltip_text((entry.label)())
                .build();
            (entry.job, button)
        })
        .collect()
}

/// Run `job` on many photos at once, as from the selection bar.
pub(super) fn run_bulk(ui: &Rc<LibraryWindowUi>, job: AssetJob, ids: Vec<String>) {
    if let Some(entry) = JOBS.iter().find(|entry| entry.job == job) {
        run(ui, job, ids, entry.queued);
    }
}

fn run(ui: &Rc<LibraryWindowUi>, job: AssetJob, ids: Vec<String>, queued: fn() -> String) {
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        match ui.ctx.api_client.run_asset_job(job, &ids).await {
            Ok(()) => {
                ui.toasts.add_toast(libadwaita::Toast::new(&queued()));
                if job == AssetJob::RegenerateThumbnail {
                    forget_thumbnails_later(&ui, ids);
                }
            }
            Err(err) => show_error_toast(
                &ui,
                &gettext("Couldn't refresh"),
                &fill(
                    gettext("Immich didn't start the job: {err}"),
                    &[("err", &err)],
                ),
            ),
        }
    });
}

fn forget_thumbnails_later(ui: &LibraryWindowUi, ids: Vec<String>) {
    let cache = ui.ctx.thumbnail_cache.clone();
    glib::timeout_add_local_once(THUMBNAIL_REFRESH_DELAY, move || {
        for id in &ids {
            cache.forget(id);
        }
    });
}
