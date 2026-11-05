//! Shared filesystem and I/O utilities.
//!
//! Provides atomic file writes (write-to-temp then rename) and other
//! small helpers used across the sync engine and configuration modules.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Atomically replace the contents of `path` with `content`.
///
/// Writes to a temporary sibling file first, then renames it into place.
/// On POSIX systems `rename(2)` is atomic within the same filesystem, so
/// readers will either see the old content or the new content -- never a
/// partially written file.
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = path.with_extension(format!("tmp.{}", nonce));

    if let Err(err) = fs::write(&tmp, content) {
        // Best-effort cleanup; the tmp file may not exist yet.
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_file_and_parent_dirs() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("sub").join("deep").join("config.json");
        atomic_write(&target, b"{\"ok\": true}").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "{\"ok\": true}");
    }

    #[test]
    fn atomic_write_replaces_existing_content() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("data.json");
        fs::write(&target, b"old").unwrap();
        atomic_write(&target, b"new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("clean.json");
        atomic_write(&target, b"data").unwrap();
        let siblings: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(siblings.len(), 1);
        assert_eq!(siblings[0].file_name().to_string_lossy(), "clean.json");
    }
}
