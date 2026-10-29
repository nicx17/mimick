//! Shared delayed loading for thumbnail pictures in library tiles and pickers.

use std::sync::Arc;

use gtk::prelude::*;

use crate::app_context::AppContext;

/// Load a thumbnail into a picture, optionally using the grid's reveal treatment.
pub(crate) fn load_into_picture(
    ctx: Arc<AppContext>,
    asset_id: String,
    picture: gtk::Picture,
    reveal: bool,
) {
    if let Some(texture) = ctx
        .thumbnail_cache
        .get_cached(&asset_id, crate::api_client::ThumbnailSize::Thumbnail)
    {
        picture.set_paintable(Some(&texture));
        return;
    }
    if reveal {
        picture.add_css_class("mimick-thumb-reveal");
        picture.add_css_class("mimick-thumb-pending");
    }
    glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
        glib::MainContext::default().spawn_local(async move {
            if let Ok(texture) = ctx
                .thumbnail_cache
                .load_thumbnail(&asset_id, crate::api_client::ThumbnailSize::Thumbnail)
                .await
            {
                picture.set_paintable(Some(&texture));
            }
            if reveal {
                picture.remove_css_class("mimick-thumb-pending");
            }
        });
    });
}
