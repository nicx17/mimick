//! Two-tier (memory + disk) thumbnail cache for the library grid.
//!
//! Remote thumbnails are fetched via the API client and decoded into
//! GDK textures. An LRU memory cache with a configurable byte budget
//! keeps hot textures in RAM, while a persistent disk cache avoids
//! redundant network requests across sessions. In-flight deduplication
//! ensures concurrent requests for the same asset coalesce into one load.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use gdk4::Texture;
use gdk4::prelude::TextureExt;
use lru::LruCache;
use tokio::sync::{Semaphore, watch};

use crate::api_client::{ImmichApiClient, ThumbnailSize};

mod decode;
use decode::{
    check_cancelled, decode_local_pixbuf, decode_to_scaled_texture, pixbuf_png_bytes,
    pixbuf_to_texture, read_cached_texture, target_dim_for_bucket,
};

const FALLBACK_CPUS: usize = 4;
const SMALL_LOAD_MAX: usize = 16;
const LARGE_LOAD_MAX: usize = 6;

type InflightSlot = Option<Result<Texture, String>>;
type InflightRx = watch::Receiver<InflightSlot>;
type InflightMap = Arc<Mutex<HashMap<String, InflightRx>>>;

/// Guard that cleans up in-flight channel entries on drop.
struct InflightGuard {
    inflight: InflightMap,
    key: String,
    tx: watch::Sender<InflightSlot>,
}

impl InflightGuard {
    /// Publish the load result to all listening subscribers.
    fn publish(&self, result: Result<Texture, String>) {
        let _ = self.tx.send(Some(result));
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        let mut map = self.inflight.lock();
        map.remove(&self.key);
    }
}

/// Helper to await an in-flight thumbnail load result from a subscriber channel.
async fn await_inflight(mut rx: InflightRx) -> Result<Texture, String> {
    if let Some(result) = rx.borrow_and_update().clone() {
        return result;
    }
    match rx.changed().await {
        Ok(()) => rx
            .borrow()
            .clone()
            .unwrap_or_else(|| Err("Thumbnail load cancelled".to_string())),
        Err(_) => Err("Thumbnail load cancelled".to_string()),
    }
}

/// LRU Cache that limits memory usage based on estimated byte size.
struct SizedLruCache {
    inner: LruCache<String, Texture>,
    current_bytes: usize,
    max_bytes: usize,
    evictions_since_log: usize,
}

impl SizedLruCache {
    /// Construct a new LRU cache with a specific byte limit.
    fn new(max_bytes: usize) -> Self {
        let approx_per_entry = 256 * 1024;
        let count_cap = (max_bytes / approx_per_entry).max(8);
        Self {
            inner: LruCache::new(NonZeroUsize::new(count_cap).unwrap()),
            current_bytes: 0,
            max_bytes,
            evictions_since_log: 0,
        }
    }

    fn stats(&mut self) -> (usize, usize, usize, usize) {
        let evictions = self.evictions_since_log;
        self.evictions_since_log = 0;
        (
            self.inner.len(),
            self.current_bytes,
            self.max_bytes,
            evictions,
        )
    }

    /// Retrieve a texture from the cache if present, updating LRU recency.
    fn get(&mut self, key: &str) -> Option<Texture> {
        self.inner.get(key).cloned()
    }

    fn peek(&self, key: &str) -> Option<Texture> {
        self.inner.peek(key).cloned()
    }

    /// Insert a texture into the cache, evicting entries to respect the byte budget.
    fn insert(&mut self, key: String, texture: Texture) {
        let added = estimate_texture_bytes(&texture);
        if let Some(previous) = self.inner.put(key, texture) {
            self.current_bytes = self
                .current_bytes
                .saturating_sub(estimate_texture_bytes(&previous));
        }
        self.current_bytes = self.current_bytes.saturating_add(added);
        while self.current_bytes > self.max_bytes {
            if let Some((_key, removed)) = self.inner.pop_lru() {
                self.current_bytes = self
                    .current_bytes
                    .saturating_sub(estimate_texture_bytes(&removed));
                self.evictions_since_log += 1;
            } else {
                break;
            }
        }
    }

    /// Clear all cached items and reset the current byte counter.
    fn clear(&mut self) {
        self.inner.clear();
        self.current_bytes = 0;
    }
}

/// A combined memory (LRU) and disk-based caching manager for remote and local assets.
pub struct ThumbnailCache {
    api_client: std::sync::Arc<ImmichApiClient>,
    memory: Mutex<SizedLruCache>,
    cache_dir: PathBuf,
    small_semaphore: Arc<Semaphore>,
    large_semaphore: Arc<Semaphore>,
    inflight: InflightMap,
}

impl ThumbnailCache {
    /// Floor for the auto-sized RAM budget, so very low-memory systems still
    /// hold a working set of decoded thumbnails.
    const AUTO_MIN_BYTES: usize = 500 * 1024 * 1024;
    const AUTO_MAX_BYTES: usize = 3 * 1024 * 1024 * 1024;
    const AUTO_FRACTION_PERCENT: usize = 20;

    /// Construct a new thumbnail cache manager. Disk pruning is handled
    /// centrally by `cache_manager` at startup, not here.
    pub fn new(api_client: std::sync::Arc<ImmichApiClient>) -> Self {
        let cache_dir = crate::profile::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp").join(crate::profile::dir_segment()))
            .join("thumbnails");

        let max_bytes = auto_memory_budget();
        let cpus = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(FALLBACK_CPUS);
        let small = SMALL_LOAD_MAX.min(cpus.saturating_mul(2)).max(2);
        let large = LARGE_LOAD_MAX.min(cpus).max(2);
        log::info!(
            "ThumbnailCache memory budget: {} MB (auto), concurrency small={} large={}",
            max_bytes / (1024 * 1024),
            small,
            large,
        );

        Self {
            api_client,
            memory: Mutex::new(SizedLruCache::new(max_bytes)),
            cache_dir,
            small_semaphore: Arc::new(Semaphore::new(small)),
            large_semaphore: Arc::new(Semaphore::new(large)),
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn new_for_test(
        api_client: std::sync::Arc<ImmichApiClient>,
        cache_dir: PathBuf,
        max_bytes: usize,
    ) -> Self {
        Self {
            api_client,
            memory: Mutex::new(SizedLruCache::new(max_bytes)),
            cache_dir,
            small_semaphore: Arc::new(Semaphore::new(SMALL_LOAD_MAX)),
            large_semaphore: Arc::new(Semaphore::new(LARGE_LOAD_MAX)),
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Retrieve a thumbnail texture from memory if present, updating LRU recency.
    pub fn get_cached(&self, asset_id: &str, size: ThumbnailSize) -> Option<Texture> {
        let key = cache_key(asset_id, size, false);
        self.memory.lock().get(&key)
    }

    /// Same as `get_cached` but does not touch LRU order. Use for read-only
    /// per-frame paint lookups.
    pub fn peek_cached(&self, asset_id: &str, size: ThumbnailSize) -> Option<Texture> {
        let key = cache_key(asset_id, size, false);
        self.memory.lock().peek(&key)
    }

    /// Preview texture from memory; edited renders are cached apart from unedited ones.
    pub fn get_cached_preview(&self, asset_id: &str, edited: bool) -> Option<Texture> {
        let key = cache_key(asset_id, ThumbnailSize::Preview, edited);
        self.memory.lock().get(&key)
    }

    /// (entries, current_bytes, max_bytes, evictions_since_last_call).
    pub fn cache_stats(&self) -> (usize, usize, usize, usize) {
        self.memory.lock().stats()
    }

    fn semaphore_for(&self, size: ThumbnailSize) -> Arc<Semaphore> {
        match size {
            ThumbnailSize::Thumbnail => self.small_semaphore.clone(),
            ThumbnailSize::Preview | ThumbnailSize::Fullsize => self.large_semaphore.clone(),
        }
    }

    /// Asynchronously fetch a remote thumbnail with default non-cancellable execution.
    pub async fn load_thumbnail(
        &self,
        asset_id: &str,
        size: ThumbnailSize,
    ) -> Result<Texture, String> {
        self.load_thumbnail_cancellable(asset_id, size, || false)
            .await
    }

    /// Asynchronously fetch a remote thumbnail with cancellable hook support.
    pub async fn load_thumbnail_cancellable<F>(
        &self,
        asset_id: &str,
        size: ThumbnailSize,
        is_cancelled: F,
    ) -> Result<Texture, String>
    where
        F: Fn() -> bool,
    {
        self.load_remote(asset_id, size, false, is_cancelled).await
    }

    /// Lightbox preview, or Immich's edited render of it when `edited` is set.
    pub async fn load_preview(&self, asset_id: &str, edited: bool) -> Result<Texture, String> {
        self.load_remote(asset_id, ThumbnailSize::Preview, edited, || false)
            .await
    }

    /// Memory cache, then in-flight request, then disk cache, then the server.
    async fn load_remote<F>(
        &self,
        asset_id: &str,
        size: ThumbnailSize,
        edited: bool,
        is_cancelled: F,
    ) -> Result<Texture, String>
    where
        F: Fn() -> bool,
    {
        let key = cache_key(asset_id, size, edited);
        if let Some(texture) = self.memory.lock().get(&key) {
            return Ok(texture);
        }

        let guard = match self.enter_inflight(&key) {
            Ok(guard) => guard,
            Err(rx) => return await_inflight(rx).await,
        };
        let result = self
            .fetch_remote_thumbnail(asset_id, size, edited, &key, &is_cancelled)
            .await;
        guard.publish(result.clone());
        result
    }

    /// Internal helper that performs the actual network request and disk backup logic.
    async fn fetch_remote_thumbnail(
        &self,
        asset_id: &str,
        size: ThumbnailSize,
        edited: bool,
        key: &str,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<Texture, String> {
        check_cancelled(is_cancelled)?;
        let _permit = self
            .semaphore_for(size)
            .acquire_owned()
            .await
            .map_err(|err| err.to_string())?;
        check_cancelled(is_cancelled)?;
        if let Some(texture) = self.memory.lock().get(key) {
            return Ok(texture);
        }

        let cache_file = self.cache_file(asset_id, size, edited);
        let cache_file_for_read = cache_file.clone();
        let from_disk =
            tokio::task::spawn_blocking(move || std::fs::read(&cache_file_for_read).ok())
                .await
                .map_err(|err| err.to_string())?;
        if let Some(bytes) = from_disk {
            return self.decode_into_memory(bytes, size, key).await;
        }

        check_cancelled(is_cancelled)?;
        let bytes = self
            .api_client
            .fetch_thumbnail(asset_id, size, edited)
            .await?;
        write_cache_bytes(self.cache_dir.clone(), cache_file, bytes.clone()).await;
        self.decode_into_memory(bytes, size, key).await
    }

    /// Decode thumbnail bytes for `size` and keep the texture in the memory cache under `key`.
    async fn decode_into_memory(
        &self,
        bytes: Vec<u8>,
        size: ThumbnailSize,
        key: &str,
    ) -> Result<Texture, String> {
        let texture = decode_to_scaled_texture(bytes, target_dim_for_bucket(size))
            .await
            .map_err(|err| err.to_string())?;
        self.memory.lock().insert(key.to_string(), texture.clone());
        Ok(texture)
    }

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
        if let Some(texture) = self.memory.lock().get(&key) {
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

    /// Decode a local file off the main thread and write the PNG thumbnail to `cache_file`.
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
            let encoded = pixbuf_png_bytes(&pixbuf)?;
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

    /// Purge all memory caches and completely delete the disk cache directory.
    pub fn clear(&self) -> Result<(), String> {
        self.memory.lock().clear();
        if self.cache_dir.exists() {
            std::fs::remove_dir_all(&self.cache_dir).map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    /// Drop every cached texture from RAM without touching the disk cache.
    /// Invoked when the library window closes so the texture memory is
    /// released until the user reopens it.
    pub fn clear_memory(&self) {
        self.memory.lock().clear();
    }

    /// Return the physical disk cache path for a remote asset thumbnail.
    fn cache_file(&self, asset_id: &str, size: ThumbnailSize, edited: bool) -> PathBuf {
        self.cache_dir.join(cache_key(asset_id, size, edited))
    }

    /// Return the physical disk cache path for a local folder asset thumbnail.
    fn cache_file_local(&self, asset_id: &str) -> PathBuf {
        self.cache_dir.join(local_cache_key(asset_id))
    }

    /// Register or join an ongoing in-flight loading request for a key.
    fn enter_inflight(&self, key: &str) -> Result<InflightGuard, InflightRx> {
        let mut map = self.inflight.lock();
        if let Some(rx) = map.get(key) {
            return Err(rx.clone());
        }
        let (tx, rx) = watch::channel::<InflightSlot>(None);
        map.insert(key.to_string(), rx);
        Ok(InflightGuard {
            inflight: self.inflight.clone(),
            key: key.to_string(),
            tx,
        })
    }
}

/// Construct a cache key string for remote assets; edited renders get their own `-edited` keys.
fn cache_key(asset_id: &str, size: ThumbnailSize, edited: bool) -> String {
    let prefix = match size {
        ThumbnailSize::Thumbnail => "thumbnail",
        ThumbnailSize::Preview => "preview",
        ThumbnailSize::Fullsize => "fullsize",
    };
    if edited {
        format!("{prefix}-edited:{asset_id}")
    } else {
        format!("{prefix}:{asset_id}")
    }
}

/// Construct a cache key string for local assets using hashed paths.
fn local_cache_key(asset_id: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    asset_id.hash(&mut hasher);
    format!("local-thumbnail-v2:{:x}", hasher.finish())
}

/// Best-effort write of downloaded thumbnail bytes to the disk cache.
async fn write_cache_bytes(cache_dir: PathBuf, cache_file: PathBuf, bytes: Vec<u8>) {
    let _ = tokio::task::spawn_blocking(move || {
        let _ = std::fs::create_dir_all(&cache_dir);
        let _ = std::fs::write(&cache_file, &bytes);
    })
    .await;
}

/// Estimate memory byte size occupied by a texture.
fn estimate_texture_bytes(texture: &Texture) -> usize {
    texture.width().max(1) as usize * texture.height().max(1) as usize * 4
}

/// Pick a thumbnail-cache RAM budget from `MemTotal` in `/proc/meminfo`,
/// clamped to `[AUTO_MIN_BYTES, AUTO_MAX_BYTES]`. Falls back to the floor
/// if the file can't be read.
fn auto_memory_budget() -> usize {
    let total = read_meminfo_total_bytes().unwrap_or(0);
    if total == 0 {
        return ThumbnailCache::AUTO_MIN_BYTES;
    }
    let fraction = total / 100 * ThumbnailCache::AUTO_FRACTION_PERCENT;
    fraction.clamp(
        ThumbnailCache::AUTO_MIN_BYTES,
        ThumbnailCache::AUTO_MAX_BYTES,
    )
}

fn read_meminfo_total_bytes() -> Option<usize> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: usize = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

#[cfg(test)]
mod tests;
