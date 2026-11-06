//! The photos either side of the current one, filled in and preloaded so a swipe shows them at once.

use gtk::prelude::*;

use super::super::strip::Slot;
use super::super::{AssetInfo, Lightbox};
use crate::api_client::ThumbnailSize;
use crate::library::LOCAL_ID_PREFIX;
use crate::library::asset_object::AssetObject;

impl Lightbox {
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
        let info = pos
            .and_then(|pos| self.ui.grid.model.item(pos))
            .and_downcast::<AssetObject>()
            .map(|item| AssetInfo::from_item(&item));
        self.fill_slot_with(slot, info);
    }

    pub(super) fn fill_slot_with(&self, slot: Slot, info: Option<AssetInfo>) {
        let strip = &self.picture.strip;
        let asset_id = info.as_ref().map_or("", |info| info.asset_id.as_str());
        if strip.asset(slot) == asset_id {
            return;
        }
        strip.set_asset(slot, asset_id);
        let texture = info.as_ref().and_then(|info| self.instant_texture(info));
        strip.picture(slot).set_paintable(texture.as_ref());
    }

    pub(super) fn instant_texture(&self, info: &AssetInfo) -> Option<gdk4::Texture> {
        let cache = &self.ui.ctx.thumbnail_cache;
        let edited = info.edited && !info.is_video();
        info.decoded_from_disk()
            .then(|| cache.cached_screen(&info.asset_id, false, self.screen_edge()))
            .flatten()
            .or_else(|| cache.get_cached_preview(&info.asset_id, edited))
            .or_else(|| cache.get_cached(&info.asset_id, ThumbnailSize::Thumbnail))
    }

    pub(in crate::library::lightbox) fn load_neighbours(&self) {
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
}
