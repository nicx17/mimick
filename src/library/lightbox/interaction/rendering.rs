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
use super::super::strip::Slot;
use super::super::{AssetInfo, Lightbox, LoadResult, PictureLoad};
use super::super::{apply_lightbox_zoom, lightbox_title_cap, rating};
use crate::api_client::ThumbnailSize;
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
        self.fill_slot(Slot::Current, Some(pos));
        self.start_picture_load(&info);
        self.fill_neighbours();
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

    fn start_picture_load(self: &Rc<Self>, info: &AssetInfo) {
        let target = self.picture.strip.picture(Slot::Current);
        self.zoom_level.set(1.0);
        apply_lightbox_zoom(&target, &self.picture.scrolled, 1.0);
        self.picture.strip.set_swipe_enabled(true);
        self.actions.zoom_reset.set_label("100%");
        self.load_into_picture(PictureLoad {
            target,
            asset_id: info.asset_id.clone(),
            filename: info.filename.clone(),
            mime: info.mime.clone(),
            local_path: info.local_path.clone(),
            full_res: self.actions.resolution_toggle.is_active(),
            is_video: info.is_video(),
            edited: info.edited,
        });
    }

    /// Reset overlays, then load the real picture asynchronously.
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
            lb.load_neighbours();
        });
    }

    pub(in crate::library::lightbox) fn fill_neighbours(&self) {
        let pos = self.pos.get();
        let next = pos
            .checked_add(1)
            .filter(|&next| next < self.ui.grid.model.n_items());
        self.fill_slot(Slot::Prev, pos.checked_sub(1));
        self.fill_slot(Slot::Next, next);
        self.picture.strip.set_neighbours(pos > 0, next.is_some());
    }

    fn fill_slot(&self, slot: Slot, pos: Option<u32>) {
        let strip = &self.picture.strip;
        let info = pos
            .and_then(|pos| self.ui.grid.model.item(pos))
            .and_downcast::<AssetObject>()
            .map(|item| AssetInfo::from_item(&item));
        let asset_id = info.as_ref().map_or("", |info| info.asset_id.as_str());
        if strip.asset(slot) == asset_id {
            return;
        }
        strip.set_asset(slot, asset_id);
        let texture = info.as_ref().and_then(|info| self.instant_texture(info));
        strip.picture(slot).set_paintable(texture.as_ref());
    }

    fn instant_texture(&self, info: &AssetInfo) -> Option<gdk4::Texture> {
        let cache = &self.ui.ctx.thumbnail_cache;
        let edited = info.edited && !info.is_video();
        info.decoded_from_disk()
            .then(|| cache.cached_screen(&info.asset_id, false, self.screen_edge()))
            .flatten()
            .or_else(|| cache.get_cached_preview(&info.asset_id, edited))
            .or_else(|| cache.get_cached(&info.asset_id, ThumbnailSize::Thumbnail))
    }

    fn load_neighbours(&self) {
        let pos = self.pos.get();
        for (slot, neighbour) in [
            (Slot::Prev, pos.checked_sub(1)),
            (Slot::Next, pos.checked_add(1)),
        ] {
            let item = neighbour
                .and_then(|pos| self.ui.grid.model.item(pos))
                .and_downcast::<AssetObject>();
            if let Some(info) = item.map(|item| AssetInfo::from_item(&item))
                && self.picture.strip.asset(slot) == info.asset_id
            {
                self.load_neighbour(self.picture.strip.picture(slot), info);
            }
        }
    }

    fn load_neighbour(&self, picture: gtk::Picture, info: AssetInfo) {
        let from_disk = info.decoded_from_disk().then(|| {
            (
                std::path::PathBuf::from(&info.local_path),
                self.screen_edge(),
            )
        });
        if from_disk.is_none() && info.asset_id.starts_with(LOCAL_ID_PREFIX) {
            return;
        }
        let cache = self.ui.ctx.thumbnail_cache.clone();
        let strip = self.picture.strip.clone();
        glib::MainContext::default().spawn_local(async move {
            let loaded = match from_disk {
                Some((path, edge)) => cache.load_screen(&info.asset_id, false, path, edge).await,
                None => {
                    let edited = info.edited && !info.is_video();
                    cache.load_preview(&info.asset_id, edited).await
                }
            };
            if let Ok(texture) = loaded
                && strip.neighbour_shows(&picture, &info.asset_id)
            {
                picture.set_paintable(Some(&texture));
            }
        });
    }

    /// Badge, unavailable card, and recognised text stay put while photos slide, so they hide.
    pub(in crate::library::lightbox) fn show_overlays(&self, shown: bool) {
        let opacity = if shown { 1.0 } else { 0.0 };
        self.video_badge_button.set_opacity(opacity);
        self.unavailable.overlay.set_opacity(opacity);
        self.picture.ocr.area.set_opacity(opacity);
    }

    fn screen_edge(&self) -> u32 {
        let monitor = self
            .ui
            .window
            .surface()
            .and_then(|surface| surface.display().monitor_at_surface(&surface));
        let edge = match monitor {
            Some(monitor) => {
                let area = monitor.geometry();
                f64::from(area.width().max(area.height())) * monitor.scale()
            }
            None => f64::from(FALLBACK_SCREEN_EDGE),
        };
        edge.ceil() as u32
    }

    async fn load_screen(
        &self,
        asset_id: &str,
        edited: bool,
        path: &std::path::Path,
    ) -> Option<gdk4::Texture> {
        self.ui
            .ctx
            .thumbnail_cache
            .load_screen(asset_id, edited, path.to_path_buf(), self.screen_edge())
            .await
            .inspect_err(|err| log::debug!("Lightbox decode failed for {}: {err}", path.display()))
            .ok()
    }

    pub(in crate::library::lightbox) fn sharpen_for_zoom(self: &Rc<Self>, content_width: f64) {
        let picture = self.active_picture();
        let shown = picture.paintable().map_or(0, |p| p.intrinsic_width());
        if content_width * f64::from(picture.scale_factor()) <= f64::from(shown) {
            return;
        }
        let Some(path) = self.full_source.borrow_mut().take() else {
            return;
        };
        let generation = self.load_gen.get();
        let cancel_loader = self.arm_loader_spinner(250);
        let lb = Rc::downgrade(self);
        glib::MainContext::default().spawn_local(async move {
            let started = std::time::Instant::now();
            let full = load_texture_oriented(&path).await;
            cancel_loader.set(true);
            let Some(lb) = lb.upgrade().filter(|lb| lb.load_gen.get() == generation) else {
                return;
            };
            lb.picture.loader.set_reveal_child(false);
            match full {
                Some(texture) => {
                    log::debug!(
                        "Lightbox sharpened to {}x{} in {}ms",
                        texture.width(),
                        texture.height(),
                        started.elapsed().as_millis()
                    );
                    picture.set_paintable(Some(&texture));
                }
                None => log::warn!("Full-size decode failed for {}", path.display()),
            }
        });
    }

    /// Clear the previous asset's overlays and drag path, and set up the video badge.
    fn prepare_for_load(&self, load: &PictureLoad) {
        self.unavailable.overlay.set_reveal_child(false);
        self.unavailable.overlay.set_can_target(false);
        *self.unavailable_path.borrow_mut() = None;
        // Clear drag path while loading; updated once resolved.
        *self.drag_path.borrow_mut() = None;
        *self.full_source.borrow_mut() = None;
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
                Ok(texture) => LoadResult::Texture {
                    texture,
                    drag_path: None,
                    full_source: None,
                },
                Err(_) => LoadResult::KeepPoster,
            };
        }
        if !load.local_path.is_empty() && !load.edited {
            let local = std::path::PathBuf::from(&load.local_path);
            if let Some(texture) = self.load_screen(&load.asset_id, false, &local).await {
                return LoadResult::Texture {
                    texture,
                    drag_path: Some(local.clone()),
                    full_source: Some(local),
                };
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
                Ok(texture) => LoadResult::Texture {
                    texture,
                    drag_path: load.edited_local_path(),
                    full_source: None,
                },
                Err(_) => LoadResult::Unavailable(None),
            }
        };
        if let LoadResult::Unavailable(_) = result
            && let Some(local) = load.edited_local_path()
            && let Some(texture) = self.load_screen(&load.asset_id, false, &local).await
        {
            return LoadResult::Texture {
                texture,
                drag_path: Some(local.clone()),
                full_source: Some(local),
            };
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
        let drag_path = if load.edited {
            load.edited_local_path()
        } else {
            drag_export_path(&load.asset_id, &load.filename, &temp)
        };
        match self.load_screen(&load.asset_id, load.edited, &temp).await {
            Some(texture) => LoadResult::Texture {
                texture,
                drag_path,
                full_source: Some(temp),
            },
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
            LoadResult::Texture {
                texture,
                drag_path,
                full_source,
            } => {
                log::debug!(
                    "Lightbox showing asset={} {}x{} decoded={}MB",
                    load.asset_id,
                    texture.width(),
                    texture.height(),
                    texture_megabytes(&texture),
                );
                load.target.set_paintable(Some(&texture));
                *self.drag_path.borrow_mut() = drag_path;
                let capped = texture.width().max(texture.height()) as u32 >= self.screen_edge();
                *self.full_source.borrow_mut() = full_source.filter(|_| capped);
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
    }

    /// A photo sharpened for zooming goes back to its screen-size copy as it slides away.
    pub(in crate::library::lightbox) fn drop_full_resolution(&self) {
        let picture = self.active_picture();
        let edge = i32::try_from(self.screen_edge()).unwrap_or(i32::MAX);
        let sharpened = picture
            .paintable()
            .is_some_and(|p| p.intrinsic_width().max(p.intrinsic_height()) > edge);
        if sharpened && let Some(item) = self.current_item() {
            let texture = self.instant_texture(&AssetInfo::from_item(&item));
            picture.set_paintable(texture.as_ref());
        }
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

const FALLBACK_SCREEN_EDGE: u32 = 2560;

fn texture_megabytes(texture: &gtk::gdk::Texture) -> usize {
    texture.width().max(0) as usize * texture.height().max(0) as usize * 4 / (1024 * 1024)
}
