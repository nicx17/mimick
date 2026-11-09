//! Thumbnails of files in watch folders: decoded here, then kept in the disk cache.

use gdk4::Texture;
use gdk4::prelude::TextureExt;
use std::path::PathBuf;

use crate::api_client::ThumbnailSize;

use super::decode::{
    check_cancelled, decode_local_pixbuf, pixbuf_cache_bytes, pixbuf_to_texture,
    read_cached_texture,
};
use super::{ThumbnailCache, await_inflight, cache_key, local_cache_key};

impl ThumbnailCache {
    /// Asynchronously generate a local file thumbnail with cancellable hook support.
    pub async fn load_local_thumbnail_cancellable<F>(
        &self,
        asset_id: &str,
        path: &std::path::Path,
        is_cancelled: F,
    ) -> Result<Texture, String>
    where
        F: Fn() -> bool,
    {
        let key = cache_key(asset_id, ThumbnailSize::Thumbnail, false);
        if let Some(texture) = self.memory.lock().lookup(&key) {
            return Ok(texture);
        }

        let guard = match self.enter_inflight(&key) {
            Ok(guard) => guard,
            Err(rx) => return await_inflight(rx).await,
        };
        let result = self
            .fetch_local_thumbnail(asset_id, path, &key, &is_cancelled)
            .await;
        guard.publish(result.clone());
        result
    }

    /// Internal helper that scales, saves, and loads a local folder asset thumbnail.
    async fn fetch_local_thumbnail(
        &self,
        asset_id: &str,
        path: &std::path::Path,
        key: &str,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<Texture, String> {
        check_cancelled(is_cancelled)?;
        let _permit = self
            .semaphore_for(ThumbnailSize::Thumbnail)
            .acquire_owned()
            .await
            .map_err(|err| err.to_string())?;
        // Checked again: the user may have scrolled away while waiting for a permit.
        check_cancelled(is_cancelled)?;
        if let Some(texture) = self.memory.lock().get(key) {
            return Ok(texture);
        }

        let cache_file = self.cache_file_local(asset_id);
        let texture = match read_cached_texture(cache_file.clone()).await? {
            Some(texture) => texture,
            None => {
                self.decode_local_to_cache(path.to_path_buf(), cache_file)
                    .await?
            }
        };
        self.memory.lock().insert(key.to_string(), texture.clone());
        Ok(texture)
    }

    /// Decode a local file off the main thread and write its thumbnail to `cache_file`.
    async fn decode_local_to_cache(
        &self,
        path: std::path::PathBuf,
        cache_file: std::path::PathBuf,
    ) -> Result<Texture, String> {
        let decode_started = std::time::Instant::now();
        let log_path = path.clone();
        let cache_dir = self.cache_dir.clone();
        let texture = tokio::task::spawn_blocking(move || -> Result<Texture, String> {
            let pixbuf = decode_local_pixbuf(&path)?;
            std::fs::create_dir_all(&cache_dir).map_err(|err| err.to_string())?;
            let encoded = pixbuf_cache_bytes(&pixbuf)?;
            std::fs::write(&cache_file, encoded).map_err(|err| err.to_string())?;
            Ok(pixbuf_to_texture(&pixbuf))
        })
        .await
        .map_err(|err| err.to_string())??;
        log::debug!(
            "Local thumbnail decoded fresh for {} in {}ms ({}x{})",
            log_path.display(),
            decode_started.elapsed().as_millis(),
            texture.width(),
            texture.height(),
        );
        Ok(texture)
    }

    pub(super) fn cache_file_local(&self, asset_id: &str) -> PathBuf {
        self.cache_dir.join(local_cache_key(asset_id))
    }
}
