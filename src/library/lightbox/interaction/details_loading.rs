//! Fills the lightbox details pane: EXIF, rating, people, tags, albums, and activity.

use std::rc::Rc;

use gtk::prelude::*;

use super::super::Lightbox;
use super::super::details::{
    fill_description, fill_exif_box, local_details_exif, probe_local_file, shows_taken_date,
};
use super::super::rating;
use crate::api_client::AssetDetails;
use crate::library::{LOCAL_ID_PREFIX, album_activity, album_assets, local_exif, trash_view};

impl Lightbox {
    /// Local image: parse EXIF on a blocking worker (cached on disk, so repeat opens are cheap).
    pub(super) fn load_local_details(self: &Rc<Self>, pos: u32, local_path: String) {
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let cache_root = local_exif::cache_root();
            let probed = tokio::task::spawn_blocking(move || {
                probe_local_file(&cache_root, std::path::Path::new(&local_path))
            })
            .await
            .ok();
            if lb.pos.get() != pos {
                return;
            }
            lb.details.loading.set_visible(false);
            // Render whatever we have. Files without an EXIF block (Unsplash,
            // screenshots, edited copies) still get the Details group populated from
            // filesystem + Pixbuf data, so the user always sees *something*.
            if let Some((exif, taken_label)) = probed.as_ref().and_then(local_details_exif) {
                fill_description(&lb.details.description, &exif, None);
                let location = fill_exif_box(&lb.details.exif, &exif, taken_label, None);
                let taken = shows_taken_date(&exif, taken_label);
                lb.details.created.set_visible(!taken);
                lb.show_location_map(location, &exif);
                lb.details.exif.set_visible(true);
            }
        });
    }

    /// Fetch EXIF from the server; dropped if the user has moved to another asset.
    pub(super) fn load_remote_details(self: &Rc<Self>, pos: u32, asset_id: String, mine: bool) {
        let on_server =
            !asset_id.starts_with(LOCAL_ID_PREFIX) && !trash_view::is_trash_active(&self.ui);
        if on_server {
            self.load_albums(pos, asset_id.clone());
            self.show_activity(&asset_id);
        }
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_asset_details(&asset_id).await;
            if lb.pos.get() != pos {
                return;
            }
            lb.details.loading.set_visible(false);
            if let Ok(details) = result {
                lb.show_remote_details(&asset_id, details, mine, on_server);
            }
        });
    }

    fn show_remote_details(
        self: &Rc<Self>,
        asset_id: &str,
        details: AssetDetails,
        mine: bool,
        on_server: bool,
    ) {
        let features = self.ui.features.get();
        if on_server && features.ratings {
            let rating = details.exif_info.as_ref().and_then(|exif| exif.rating);
            let rating = rating::stars_from_exif(rating);
            self.details.rating.show(asset_id, rating, mine);
        }
        if mine && features.people {
            let taken = details.local_date_time.as_deref();
            self.details.people.show(&self.ui, &details.people, taken);
        }
        if on_server && features.tags {
            self.details.tags.show(asset_id, details.tags);
        }
        let exif = details
            .exif_info
            .or_else(|| mine.then(crate::api_client::ExifInfo::default));
        if let Some(exif) = exif {
            self.show_remote_exif(&exif, mine);
        }
    }

    fn show_remote_exif(self: &Rc<Self>, exif: &crate::api_client::ExifInfo, mine: bool) {
        let edit = mine.then(|| self.set_location_button());
        let add_description = mine.then(|| self.add_description_row());
        fill_description(&self.details.description, exif, add_description);
        let location = fill_exif_box(&self.details.exif, exif, "Taken", edit);
        self.details
            .created
            .set_visible(!shows_taken_date(exif, "Taken"));
        self.show_location_map(location, exif);
        self.details.exif.set_visible(true);
    }

    fn show_activity(&self, asset_id: &str) {
        let album = album_assets::current_album(&self.ui).filter(album_activity::has_activity);
        if let Some(album) = album {
            self.details.activity.show(&self.ui, &album, asset_id);
        }
    }

    fn load_albums(self: &Rc<Self>, pos: u32, asset_id: String) {
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.albums_containing(&asset_id).await;
            if lb.pos.get() != pos {
                return;
            }
            if let Ok(albums) = result {
                lb.details.albums.show(&lb.ui, &albums);
            }
        });
    }
}
