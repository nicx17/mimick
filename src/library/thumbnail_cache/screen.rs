//! Lightbox pictures decoded from local files to the screen's size, kept with the previews.

use gdk4::Texture;
use gettextrs::gettext;
use std::path::{Path, PathBuf};

use super::{ThumbnailCache, await_inflight};

impl ThumbnailCache {
    pub fn cached_screen(&self, asset_id: &str, edited: bool, edge: u32) -> Option<Texture> {
        self.memory.lock().get(&screen_key(asset_id, edited, edge))
    }

    /// A lightbox picture decoded from `path` to fit `edge` pixels, cached with the previews.
    pub async fn load_screen(
        &self,
        asset_id: &str,
        edited: bool,
        path: PathBuf,
        edge: u32,
    ) -> Result<Texture, String> {
        let key = screen_key(asset_id, edited, edge);
        if let Some(texture) = self.memory.lock().lookup(&key) {
            return Ok(texture);
        }
        let guard = match self.enter_inflight(&key) {
            Ok(guard) => guard,
            Err(rx) => return await_inflight(rx).await,
        };
        let look = self.raw_look(asset_id, edited, &path).await;
        let decoded = tokio::task::spawn_blocking(move || {
            crate::library::load_texture_fitted_blocking(&path, edge, look.as_ref())
        })
        .await;
        let result = match decoded {
            Ok(Some(texture)) => {
                self.memory.lock().insert(key, texture.clone());
                Ok(texture)
            }
            Ok(None) => Err(gettext("No decoder could read the file")),
            Err(err) => Err(err.to_string()),
        };
        guard.publish(result.clone());
        result
    }

    /// Immich's preview of a synced RAW that full decoding develops: the picture the grid
    /// shows, whose look the decode takes so the two match. `None` for anything else.
    pub async fn raw_look(&self, asset_id: &str, edited: bool, path: &Path) -> Option<Texture> {
        if asset_id.is_empty()
            || asset_id.starts_with(crate::library::LOCAL_ID_PREFIX)
            || !crate::library::develops_raw(path)
        {
            return None;
        }
        self.load_preview(asset_id, edited).await.ok()
    }
}

pub(super) fn screen_key(asset_id: &str, edited: bool, edge: u32) -> String {
    let edited = if edited { "-edited" } else { "" };
    format!("screen{edited}@{edge}:{asset_id}")
}
