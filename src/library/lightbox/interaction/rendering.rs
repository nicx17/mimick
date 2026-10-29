use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use libadwaita::prelude::*;

use crate::library::actions::is_mine;
use crate::library::asset_object::AssetObject;
use crate::library::download::{begin_download_session, finish_download_item, track_download_item};
use crate::library::local_exif;

use super::super::details::truncate_filename;
use super::super::details::{
    drag_export_path, edited_original_cache_path, fill_exif_box, format_datetime_display,
    local_details_exif, original_preview_cache_path, probe_local_file,
};
use super::super::{AssetInfo, Lightbox, LoadResult, PictureLoad};
use super::super::{apply_lightbox_zoom, lightbox_title_cap, rating};
use crate::library::{LOCAL_ID_PREFIX, load_texture_oriented, timeline, trash_view};

impl Lightbox {
    pub(in crate::library::lightbox) fn render(self: &Rc<Self>) {
        let pos = self.pos.get();
        let Some(item) = self.ui.grid.model.item(pos).and_downcast::<AssetObject>() else {
            return;
        };
        let info = AssetInfo::from_item(&item);

        self.header.page.set_title(&truncate_filename(
            &info.filename,
            lightbox_title_cap(&self.ui),
        ));
        self.show_summary(&info);
        let more_before = timeline::has_unloaded_neighbor(&self.ui, pos, false);
        let more_after = timeline::has_unloaded_neighbor(&self.ui, pos, true);
        self.header.prev_btn.set_sensitive(pos > 0 || more_before);
        self.header
            .next_btn
            .set_sensitive(pos + 1 < self.ui.grid.model.n_items() || more_after);

        let show_remote_actions = !info.is_local() && !info.is_video();
        self.actions
            .resolution_toggle
            .set_visible(show_remote_actions);
        self.actions.download.set_visible(show_remote_actions);
        let on_server = !info.asset_id.starts_with(LOCAL_ID_PREFIX);
        let in_trash = trash_view::is_trash_active(&self.ui);
        self.actions.trash.set_visible(on_server && !in_trash);
        self.actions.favorite.set_visible(on_server && !in_trash);
        self.show_heart(info.favorite);
        self.actions.archive.set_visible(on_server && !in_trash);
        self.show_archive(info.archived);
        let editable = on_server && !in_trash && is_mine(&self.ui, &item);
        self.details.edit_btn.set_visible(editable);
        self.details.location_btn.set_visible(editable);
        self.details.album_btn.set_visible(on_server && !in_trash);
        self.actions.zoom_group.set_visible(!info.is_video());
        self.actions.open_with.set_visible(info.is_video());

        self.prepare_video(info.is_video());
        self.picture.ocr.clear();
        self.sync_ocr_controls();
        self.start_picture_load(&info);
        self.details.loading.set_visible(true);
        if info.is_local() && !info.is_video() {
            self.load_local_details(pos, info.local_path);
        } else {
            self.load_remote_details(pos, info.asset_id, editable);
        }
    }

    /// Filename and sync summary; clears the previous asset's EXIF rows.
    fn show_summary(&self, info: &AssetInfo) {
        self.details.filename.set_label(&info.filename);
        let sync_label = match info.sync_state {
            2 => "On Immich and locally",
            1 => "Local only",
            _ => "On Immich only",
        };
        self.details.summary.set_label(&format!(
            "{} · {}\nCreated: {}",
            info.mime,
            sync_label,
            format_datetime_display(&info.created)
        ));
        while let Some(c) = self.details.exif.first_child() {
            self.details.exif.remove(&c);
        }
        self.details.exif.set_visible(false);
        self.details.rating.clear();
        self.details.people.clear();
        self.details.tags.clear();
        self.details.albums.clear();
    }

    /// Load into the *inactive* picture and commit the slide transition only after
    /// the texture is set, so the user keeps seeing the current image (with the
    /// loader spinner) until the new one is actually ready.
    fn start_picture_load(self: &Rc<Self>, info: &AssetInfo) {
        let target_is_a = !self.active_a.get();
        let target = if target_is_a {
            self.picture.picture_a.clone()
        } else {
            self.picture.picture_b.clone()
        };
        self.zoom_level.set(1.0);
        apply_lightbox_zoom(&target, &self.picture.scrolled, 1.0);
        self.actions.zoom_reset.set_label("100%");
        self.picture
            .stack
            .set_transition_type(match self.nav_dir.get() {
                1 => gtk::StackTransitionType::SlideLeft,
                -1 => gtk::StackTransitionType::SlideRight,
                _ => gtk::StackTransitionType::None,
            });
        self.load_into_picture(PictureLoad {
            target,
            target_is_a,
            asset_id: info.asset_id.clone(),
            filename: info.filename.clone(),
            mime: info.mime.clone(),
            local_path: info.local_path.clone(),
            full_res: self.actions.resolution_toggle.is_active(),
            is_video: info.is_video(),
            edited: info.edited,
        });
        // The slide direction applies to this navigation only.
        self.nav_dir.set(0);
    }

    /// Reset overlays, show any cached thumbnail, then load the real picture asynchronously.
    fn load_into_picture(self: &Rc<Self>, load: PictureLoad) {
        let generation = self.load_gen.get().wrapping_add(1);
        self.load_gen.set(generation);
        self.prepare_for_load(&load);
        // Reveal the spinner after a short delay so fast cache hits and
        // quick JPEG decodes don't flash it. Local paths get a longer
        // delay since most JPEGs decode in well under 250ms, but RAW
        // and large TIFF decodes can run for seconds and need feedback.
        let delay_ms = if load.local_path.is_empty() { 120 } else { 250 };
        let cancel_loader = self.arm_loader_spinner(delay_ms);

        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.fetch_picture(&load).await;
            if lb.load_gen.get() != generation {
                return;
            }
            lb.show_load_result(&load, result);
            cancel_loader.set(true);
            lb.picture.loader.set_reveal_child(false);
            lb.prefetch_neighbors();
        });
    }

    fn prefetch_neighbors(&self) {
        if self.actions.resolution_toggle.is_active() {
            return;
        }
        let pos = self.pos.get();
        for neighbor in [pos.checked_sub(1), pos.checked_add(1)]
            .into_iter()
            .flatten()
        {
            let Some(item) = self
                .ui
                .grid
                .model
                .item(neighbor)
                .and_downcast::<AssetObject>()
            else {
                continue;
            };
            let info = AssetInfo::from_item(&item);
            let decoded_from_disk = !info.local_path.is_empty() && !info.edited;
            if info.is_video() || decoded_from_disk || info.asset_id.starts_with(LOCAL_ID_PREFIX) {
                continue;
            }
            let cache = self.ui.ctx.thumbnail_cache.clone();
            glib::MainContext::default().spawn_local(async move {
                let _ = cache.load_preview(&info.asset_id, info.edited).await;
            });
        }
    }

    /// Clear the previous asset's overlays and drag path, set up the video badge,
    /// and show the cached grid thumbnail until the real picture arrives.
    fn prepare_for_load(&self, load: &PictureLoad) {
        self.unavailable.overlay.set_reveal_child(false);
        self.unavailable.overlay.set_can_target(false);
        *self.unavailable_path.borrow_mut() = None;
        // Clear drag path while loading; updated once resolved.
        *self.drag_path.borrow_mut() = None;
        log::debug!(
            "Lightbox load: asset={} file={:?} mime={} source={} full_res={} kind={}",
            load.asset_id,
            load.filename,
            load.mime,
            if load.local_path.is_empty() {
                "remote"
            } else {
                "local"
            },
            load.full_res,
            if load.is_video { "video" } else { "image" },
        );
        // Videos use the still thumbnail as a poster + play badge; image
        // decoders would all fail and fall through to "unavailable".
        self.video_badge_button.set_visible(load.is_video);
        // Show the cached grid thumbnail immediately while the real picture loads.
        if let Some(texture) = self
            .ui
            .ctx
            .thumbnail_cache
            .get_cached_preview(&load.asset_id, load.edited)
        {
            load.target.set_paintable(Some(&texture));
        }
    }

    /// Reveal the spinner after `delay_ms` unless the returned flag is set first.
    fn arm_loader_spinner(&self, delay_ms: u64) -> Rc<Cell<bool>> {
        let loader_for_arm = self.picture.loader.clone();
        // Set once the load finishes so the delayed spinner never appears.
        let cancel_loader = Rc::new(Cell::new(false));
        let cancel_for_arm = cancel_loader.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), move || {
            if !cancel_for_arm.get() {
                loader_for_arm.set_reveal_child(true);
            }
            glib::ControlFlow::Break
        });
        cancel_loader
    }

    /// Pick the source: video poster, local file, original download, or preview thumbnail.
    /// Edited assets use Immich's edited render and fall back to the local file.
    async fn fetch_picture(&self, load: &PictureLoad) -> LoadResult {
        if load.is_video {
            // Use the grid's preview thumbnail as the poster.
            return match self.load_preview_thumbnail(&load.asset_id, false).await {
                Ok(texture) => LoadResult::Texture(texture, None),
                Err(_) => LoadResult::KeepPoster,
            };
        }
        if !load.local_path.is_empty() && !load.edited {
            let local = std::path::PathBuf::from(&load.local_path);
            if let Some(texture) = load_texture_oriented(&local).await {
                return LoadResult::Texture(texture, Some(local));
            }
            if load.asset_id.starts_with(LOCAL_ID_PREFIX) {
                return LoadResult::Unavailable(Some(load.local_path.clone()));
            }
        }
        let result = if load.full_res {
            self.fetch_original(load).await
        } else {
            match self
                .load_preview_thumbnail(&load.asset_id, load.edited)
                .await
            {
                Ok(texture) => LoadResult::Texture(texture, load.edited_local_path()),
                Err(_) => LoadResult::Unavailable(None),
            }
        };
        if let LoadResult::Unavailable(_) = result
            && let Some(local) = load.edited_local_path()
            && let Some(texture) = load_texture_oriented(&local).await
        {
            return LoadResult::Texture(texture, Some(local));
        }
        result
    }

    async fn load_preview_thumbnail(
        &self,
        asset_id: &str,
        edited: bool,
    ) -> Result<gdk4::Texture, String> {
        self.ui
            .ctx
            .thumbnail_cache
            .load_preview(asset_id, edited)
            .await
    }

    /// Original file from the preview cache, downloaded on a miss.
    async fn fetch_original(&self, load: &PictureLoad) -> LoadResult {
        let Some(cache_dir) = crate::profile::cache_dir().map(|p| p.join("preview")) else {
            return LoadResult::Unavailable(None);
        };
        let _ = tokio::fs::create_dir_all(&cache_dir).await;
        let temp = if load.edited {
            edited_original_cache_path(&cache_dir, &load.asset_id)
        } else {
            original_preview_cache_path(&cache_dir, &load.asset_id, &load.filename)
        };
        if temp.exists() {
            log::debug!(
                "Lightbox original cache hit for {} at {}",
                load.asset_id,
                temp.display(),
            );
        } else if let Err(err) = self
            .download_original(&load.asset_id, &temp, load.edited)
            .await
        {
            log::warn!("Lightbox original fetch failed: {}", err);
            return LoadResult::Unavailable(None);
        }
        match load_texture_oriented(&temp).await {
            Some(texture) if load.edited => LoadResult::Texture(texture, load.edited_local_path()),
            Some(texture) => LoadResult::Texture(
                texture,
                drag_export_path(&load.asset_id, &load.filename, &temp),
            ),
            None if load.edited => LoadResult::Unavailable(None),
            None => LoadResult::Unavailable(Some(temp.display().to_string())),
        }
    }

    /// Download an original, registered with the download progress tracker.
    async fn download_original(
        &self,
        asset_id: &str,
        temp: &std::path::Path,
        edited: bool,
    ) -> Result<(), String> {
        let ctx = &self.ui.ctx;
        let download_started = std::time::Instant::now();
        begin_download_session(ctx, format!("preview {asset_id}"));
        let progress = track_download_item(
            ctx,
            asset_id.to_string(),
            Some(format!("preview {asset_id}")),
            None,
        );
        let result = if edited {
            ctx.api_client
                .download_edited_original_to_file(asset_id, temp, Some(progress))
                .await
        } else {
            ctx.api_client
                .download_original_to_file(asset_id, temp, Some(progress))
                .await
        };
        finish_download_item(ctx, asset_id);
        result?;
        let downloaded_bytes = tokio::fs::metadata(temp)
            .await
            .as_ref()
            .map(std::fs::Metadata::len)
            .unwrap_or(0);
        log::debug!(
            "Lightbox original downloaded for {} in {}ms ({} bytes)",
            asset_id,
            download_started.elapsed().as_millis(),
            downloaded_bytes,
        );
        Ok(())
    }

    /// Apply a load result, then slide to the target picture.
    fn show_load_result(self: &Rc<Self>, load: &PictureLoad, result: LoadResult) {
        match result {
            LoadResult::Texture(texture, drag_path) => {
                load.target.set_paintable(Some(&texture));
                *self.drag_path.borrow_mut() = drag_path;
                if !load.is_video
                    && !load.asset_id.starts_with(LOCAL_ID_PREFIX)
                    && self.ui.features.get().ocr
                {
                    self.load_ocr(
                        load.asset_id.clone(),
                        (f64::from(texture.width()), f64::from(texture.height())),
                    );
                }
            }
            LoadResult::Unavailable(path) => {
                self.unavailable.filename.set_label(&load.filename);
                self.unavailable.mime.set_label(&load.mime);
                self.unavailable.open.set_visible(path.is_some());
                *self.unavailable_path.borrow_mut() = path;
                self.unavailable.overlay.set_can_target(true);
                self.unavailable.overlay.set_reveal_child(true);
            }
            LoadResult::KeepPoster => {}
        }
        // Defer the child switch by one idle so the target picture
        // re-measures with the new texture before the slide starts.
        let pic_stack = self.picture.stack.clone();
        let target_is_a = load.target_is_a;
        let lb = self.clone();
        glib::idle_add_local_once(move || {
            pic_stack.set_visible_child_name(if target_is_a { "a" } else { "b" });
            lb.active_a.set(target_is_a);
        });
    }

    /// Local image: parse EXIF on a blocking worker (cached on disk, so repeat opens are cheap).
    fn load_local_details(self: &Rc<Self>, pos: u32, local_path: String) {
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
            // screenshots, edited copies) still get the Image group populated from
            // filesystem + Pixbuf data, so the user always sees *something*.
            if let Some((exif, taken_label)) = probed.as_ref().and_then(local_details_exif) {
                let location = fill_exif_box(&lb.details.exif, &exif, taken_label);
                lb.show_location_map(location, &exif);
                lb.details.exif.set_visible(true);
            }
        });
    }

    /// Fetch EXIF from the server; dropped if the user has moved to another asset.
    fn load_remote_details(self: &Rc<Self>, pos: u32, asset_id: String, mine: bool) {
        if !asset_id.starts_with(LOCAL_ID_PREFIX) && !trash_view::is_trash_active(&self.ui) {
            self.load_albums(pos, asset_id.clone());
        }
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = lb.ui.ctx.api_client.fetch_asset_details(&asset_id).await;
            if lb.pos.get() != pos {
                return;
            }
            lb.details.loading.set_visible(false);
            let Ok(details) = result else {
                return;
            };
            let on_server =
                !asset_id.starts_with(LOCAL_ID_PREFIX) && !trash_view::is_trash_active(&lb.ui);
            let features = lb.ui.features.get();
            if on_server && features.ratings {
                let rating = details.exif_info.as_ref().and_then(|exif| exif.rating);
                let rating = rating::stars_from_exif(rating);
                lb.details.rating.show(&asset_id, rating, mine);
            }
            if mine && features.people {
                let taken = details.local_date_time.as_deref();
                lb.details.people.show(&lb.ui, &details.people, taken);
            }
            if on_server && features.tags {
                lb.details.tags.show(&asset_id, details.tags);
            }
            if let Some(exif) = details.exif_info {
                let location = fill_exif_box(&lb.details.exif, &exif, "Taken");
                lb.show_location_map(location, &exif);
                lb.details.exif.set_visible(true);
            }
        });
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
