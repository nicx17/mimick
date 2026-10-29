//! Unified prune + clear over every on-disk cache subdirectory.

use std::path::PathBuf;
use std::time::SystemTime;

/// Cache subdirectories under `profile::cache_dir()`. Append to extend.
pub const CACHE_SUBDIRS: &[&str] = &[
    "thumbnails",
    "raw_decode",
    "exif",
    "video",
    "preview",
    "open-in",
    "drag_export",
];

/// Yield to the scheduler every N file ops during a prune sweep.
const YIELD_EVERY: usize = 64;

fn cache_root() -> Option<PathBuf> {
    crate::profile::cache_dir()
}

/// Remove every managed cache subdir. Call from `spawn_blocking`.
pub fn clear_all_blocking() -> Result<(), String> {
    let Some(root) = cache_root() else {
        return Ok(());
    };
    let mut first_error: Option<String> = None;
    for sub in CACHE_SUBDIRS {
        let dir = root.join(sub);
        if !dir.exists() {
            continue;
        }
        if let Err(err) = std::fs::remove_dir_all(&dir)
            && first_error.is_none()
        {
            first_error = Some(format!("{}: {}", dir.display(), err));
        }
    }
    match first_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Bytes used by every managed cache subdir. Call from `spawn_blocking`.
pub fn total_size_blocking() -> u64 {
    let Some(root) = cache_root() else {
        return 0;
    };
    let (mut entries, mut total) = (Vec::new(), 0);
    for sub in CACHE_SUBDIRS {
        collect_files(&root.join(sub), &mut entries, &mut total);
    }
    total
}

/// LRU-evict files across all subdirs until total size ≤ `cap_bytes`.
pub fn prune_all_blocking(cap_bytes: u64) {
    let Some(root) = cache_root() else {
        return;
    };
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    for sub in CACHE_SUBDIRS {
        collect_files(&root.join(sub), &mut entries, &mut total);
    }
    if total <= cap_bytes {
        return;
    }
    entries.sort_by_key(|(_, _, mtime)| *mtime);
    let mut count: usize = 0;
    for (path, size, _) in entries {
        if total <= cap_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
        count += 1;
        if count.is_multiple_of(YIELD_EVERY) {
            std::thread::yield_now();
        }
    }
}

fn collect_files(
    dir: &std::path::Path,
    out: &mut Vec<(PathBuf, u64, SystemTime)>,
    total: &mut u64,
) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            collect_files(&path, out, total);
            continue;
        }
        let size = metadata.len();
        let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        *total = total.saturating_add(size);
        out.push((path, size, mtime));
    }
}
