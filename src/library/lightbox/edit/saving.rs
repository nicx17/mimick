//! Saving: crops, turns, and flips go to Immich as edits of the photo itself. Anything else is
//! drawn from the original at full size and uploaded as a new photo, stacked over the original
//! and put in its albums.

use std::rc::Rc;

use gettextrs::gettext;
use gtk::prelude::*;

use super::super::Lightbox;
use super::super::details::edited_original_cache_path;
use crate::api_client::{CopyUpload, ImmichApiClient, LibraryAsset};
use crate::library::asset_object::AssetObject;
use crate::library::context_menu::{ensure_original_asset_path, show_error_toast};
use crate::library::photo_editor::{CopyDetails, CopyJob, EditSession, match_record};

/// How often to ask whether Immich has rendered a saved edit, and how many times.
const RENDER_POLL: std::time::Duration = std::time::Duration::from_millis(1500);
const RENDER_POLLS: u32 = 40;

impl Lightbox {
    pub(super) fn save_edit(self: &Rc<Self>) {
        let copy = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(EditSession::needs_copy);
        match copy {
            Some(true) => self.save_copy(),
            Some(false) => self.save_immich_edit(),
            None => {}
        }
    }

    fn save_immich_edit(self: &Rc<Self>) {
        let Some((id, edits, thumbhash)) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(|s| (s.asset_id.clone(), s.edits(), s.thumbhash.clone()))
        else {
            return;
        };
        self.edit_mode.widgets.save.set_sensitive(false);
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let api = &lb.ui.ctx.api_client;
            let result = if edits.is_empty() {
                api.revert_edits(&id).await
            } else {
                api.save_edits(&id, &edits).await
            };
            lb.finish_edit(result, id, !edits.is_empty(), thumbhash);
        });
    }

    pub(super) fn confirm_revert_edit(self: &Rc<Self>) {
        let Some((id, thumbhash)) = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .filter(|s| s.was_edited())
            .map(|s| (s.asset_id.clone(), s.thumbhash.clone()))
        else {
            return;
        };
        let lb = self.clone();
        crate::library::dialogs::confirm(
            &self.ui,
            &gettext("Revert to the Original?"),
            &gettext("Every crop, turn, and flip made in Immich is removed."),
            &gettext("Revert"),
            move || {
                let (lb, id, thumbhash) = (lb.clone(), id.clone(), thumbhash.clone());
                glib::MainContext::default().spawn_local(async move {
                    let result = lb.ui.ctx.api_client.revert_edits(&id).await;
                    lb.finish_edit(result, id, false, thumbhash);
                });
            },
        );
    }

    /// Immich renders the edit in the background; the photo refreshes once that's done.
    fn finish_edit(
        self: &Rc<Self>,
        result: Result<(), String>,
        id: String,
        edited: bool,
        thumbhash: Option<String>,
    ) {
        if let Err(err) = result {
            self.sync_edit_save();
            show_error_toast(&self.ui, &gettext("Immich didn't save the edit"), &err);
            return;
        }
        if let Some(item) = self.edit_mode.item.borrow().as_ref() {
            item.set_is_edited(edited);
        }
        self.exit_edit_mode();
        self.ui.toasts.add_toast(libadwaita::Toast::new(&gettext(
            "Saved. Immich is updating the photo.",
        )));
        let (lb, api) = (Rc::downgrade(self), self.ui.ctx.api_client.clone());
        glib::MainContext::default().spawn_local(async move {
            wait_for_render(&api, &id, thumbhash.as_deref()).await;
            if let Some(lb) = lb.upgrade() {
                lb.show_rendered_edit(&id);
            }
        });
    }

    /// Drops every copy of the old render, then redraws the grid and, if still shown, the photo.
    fn show_rendered_edit(self: &Rc<Self>, id: &str) {
        self.ui.ctx.thumbnail_cache.forget(id);
        if let Some(dir) = crate::profile::cache_dir() {
            let _ = std::fs::remove_file(edited_original_cache_path(&dir.join("preview"), id));
        }
        self.ui.grid.canvas.queue_draw();
        let still_here = self.current_item().is_some_and(|item| item.id() == id);
        if still_here && !self.edit_mode.is_active() {
            self.render();
        }
    }

    /// Draws the original at full size with every edit and uploads it as a new photo.
    fn save_copy(self: &Rc<Self>) {
        let keep = self.edit_mode.widgets.keep_details.is_active();
        let stack = self.edit_mode.widgets.stack_copy.is_active();
        let job = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .map(|s| (s.copy_job(keep), s.asset_id.clone(), s.asset.clone()));
        let item = self.edit_mode.item.borrow().clone();
        let (Some((job, id, asset)), Some(item)) = (job, item) else {
            return;
        };
        self.edit_mode.saving.set(true);
        self.sync_edit_save();
        let kept = job.keeps_details().then(|| job.details().clone());
        let lb = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let made = lb.make_copy(&item, job, asset.as_ref()).await;
            lb.edit_mode.saving.set(false);
            match made {
                Ok(copy) => lb.copy_saved(&id, &copy, kept.as_ref(), stack).await,
                Err(err) => {
                    lb.sync_edit_save();
                    show_error_toast(&lb.ui, &gettext("Couldn't save the copy"), &err);
                }
            }
        });
    }

    /// The original, in whatever format, decoded, developed, encoded, and uploaded; a new
    /// photo is dated now, and isn't a favorite.
    async fn make_copy(
        &self,
        item: &AssetObject,
        job: CopyJob,
        asset: Option<&LibraryAsset>,
    ) -> Result<String, String> {
        let filename = item.property::<String>("filename");
        let path = ensure_original_asset_path(
            &self.ui,
            &item.property::<String>("id"),
            &item.property::<String>("remote-id"),
            &item.property::<String>("local-path"),
            &filename,
        )
        .await?;
        let texture = crate::library::load_texture_oriented(&path)
            .await
            .ok_or_else(|| gettext("Couldn't open the original"))?;
        let (pixels, stride, size) = texture_pixels(&texture);
        drop(texture);
        let asset = asset.filter(|_| job.keeps_details());
        let job = job.with_original(&path);
        let copy = tokio::task::spawn_blocking(move || job.run(&pixels, stride, size))
            .await
            .map_err(|err| err.to_string())??;
        let upload = CopyUpload {
            filename: copy_name(&filename, copy.extension),
            bytes: copy.bytes,
            mime: copy.mime,
            created_at: asset
                .map_or_else(|| chrono::Utc::now().to_rfc3339(), |a| a.created_at.clone()),
            favorite: asset.is_some_and(|a| a.is_favorite),
        };
        self.ui.ctx.api_client.upload_copy(upload).await
    }

    /// Gives the copy the original's record and albums when it keeps its details, and stacks
    /// it when asked, then leaves the editor; the grid catches up on close.
    async fn copy_saved(
        self: &Rc<Self>,
        original: &str,
        copy: &str,
        kept: Option<&CopyDetails>,
        stack: bool,
    ) {
        let placed = place_copy(&self.ui.ctx.api_client, original, copy, kept, stack).await;
        self.reload_on_close.set(true);
        // Only the edit that was saved closes, not one started on another photo meanwhile.
        let same = self
            .edit_mode
            .session
            .borrow()
            .as_ref()
            .is_some_and(|s| s.asset_id == original);
        if same {
            self.exit_edit_mode();
        }
        let done = match (placed, stack) {
            (Err(err), _) => {
                let title = gettext("Saved as a new photo, but not everything carried over");
                show_error_toast(&self.ui, &title, &err);
                return;
            }
            (Ok(()), true) => gettext("Saved as a new photo, stacked with the original"),
            (Ok(()), false) => gettext("Saved as a new photo"),
        };
        self.ui.toasts.add_toast(libadwaita::Toast::new(&done));
    }
}

/// The texture's pixels as straight RGBA, with the bytes per row and the size.
fn texture_pixels(texture: &gtk::gdk::Texture) -> (glib::Bytes, usize, (usize, usize)) {
    let mut downloader = gtk::gdk::TextureDownloader::new(texture);
    downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let size = (texture.width() as usize, texture.height() as usize);
    (bytes, stride, size)
}

/// With the original's details kept, gives the copy its record in Immich and its albums;
/// with `stack`, makes the copy the cover of a stack over the original.
async fn place_copy(
    api: &ImmichApiClient,
    original: &str,
    copy: &str,
    kept: Option<&CopyDetails>,
    stack: bool,
) -> Result<(), String> {
    let copy = copy.to_string();
    if let Some(details) = kept {
        match_record(api, &copy, details).await?;
        for album in api.albums_containing(original).await? {
            if !api
                .add_assets_to_album(&album.id, std::slice::from_ref(&copy))
                .await
            {
                return Err(gettext("Couldn't add it to the original's albums"));
            }
        }
    }
    if stack {
        api.create_stack(&[copy, original.to_string()]).await?;
    }
    Ok(())
}

/// "IMG_1.HEIC" saved as a JPEG copy is "IMG_1_edited.jpg".
fn copy_name(original: &str, extension: &str) -> String {
    let stem = std::path::Path::new(original)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "photo".to_string());
    format!("{stem}_edited.{extension}")
}

/// Immich's render job changes the thumbhash as its last step; gives up after a minute.
async fn wait_for_render(api: &ImmichApiClient, id: &str, before: Option<&str>) {
    for _ in 0..RENDER_POLLS {
        glib::timeout_future(RENDER_POLL).await;
        match api.fetch_asset_by_id(id).await {
            Ok(asset) if asset.thumbhash.as_deref() == before => {}
            Ok(_) => return,
            Err(err) => {
                log::debug!("Stopped waiting for the edit of {id}: {err}");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_named_after_the_original() {
        assert_eq!(copy_name("IMG_1.HEIC", "jpg"), "IMG_1_edited.jpg");
        assert_eq!(copy_name("scan.tar.png", "png"), "scan.tar_edited.png");
        assert_eq!(copy_name("", "jpg"), "photo_edited.jpg");
    }
}
