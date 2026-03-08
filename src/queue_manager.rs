use tokio::sync::{mpsc, Mutex};
use std::sync::Arc;
use std::path::PathBuf;
use std::fs;
use crate::api_client::ImmichApiClient;
use crate::state_manager::{StateManager, AppState};
use crate::notifications;

/// A file task with path, checksum, and optional album association (from per-path config).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileTask {
    pub path: String,
    pub checksum: String,
    /// Optional album ID (if already resolved)
    #[serde(default)]
    pub album_id: Option<String>,
    /// Album name to look up or create
    #[serde(default)]
    pub album_name: Option<String>,
}

pub struct QueueManager {
    sender: mpsc::Sender<FileTask>,
    total_queued: Arc<Mutex<usize>>,
    #[allow(dead_code)]
    pub state_manager: Arc<StateManager>,
}

impl QueueManager {
    pub fn new(api_client: Arc<ImmichApiClient>, workers: usize) -> Self {
        let (tx, rx) = mpsc::channel::<FileTask>(1000);
        let rx = Arc::new(Mutex::new(rx));
        let state_manager = Arc::new(StateManager::new());

        let processed_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let total_queued: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let active_workers: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));

        let retry_path = {
            let mut p = dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("~/.cache"))
                .join("mimick");
            p.push("retries.json");
            p
        };

        let qm = Self {
            sender: tx,
            total_queued: total_queued.clone(),
            state_manager: state_manager.clone(),
        };

        // Load persisted retries from previous session, then clear file so it
        // doesn't snowball across restarts. Items are re-added only if they fail again.
        let loaded_retries = load_retries(&retry_path);
        if !loaded_retries.is_empty() {
            log::info!("Loaded {} item(s) from retry queue. Clearing file.", loaded_retries.len());
            // Clear immediately — failures this session will re-populate it
            let _ = std::fs::write(&retry_path, "[]");
        }

        for i in 0..workers {
            let rx_clone = rx.clone();
            let api = api_client.clone();
            let sm = state_manager.clone();
            let p_count = processed_count.clone();
            let t_queued = total_queued.clone();
            let active = active_workers.clone();
            let retry_path_clone = retry_path.clone();

            tokio::spawn(async move {
                log::debug!("Worker {} started", i);
                loop {
                    let task = {
                        let mut receiver = rx_clone.lock().await;
                        receiver.recv().await
                    };

                    match task {
                        Some(file_task) => {
                            {
                                let mut a = active.lock().await;
                                *a += 1;
                            }

                            let pc = { *p_count.lock().await };
                            let tq = { *t_queued.lock().await };
                            let progress = if tq > 0 { ((pc as f32 / tq as f32) * 100.0) as u8 } else { 0 };

                            log::info!("Worker {} uploading [{}/{}]: {}", i, pc + 1, tq, file_task.path);

                            sm.write_state(AppState {
                                queue_size: tq.saturating_sub(pc),
                                total_queued: tq,
                                processed_count: pc,
                                current_file: Some(file_task.path.clone()),
                                status: "uploading".to_string(),
                                progress,
                                timestamp: 0.0,
                            });

                            let t_start = std::time::Instant::now();
                            let success = handle_upload(&api, &file_task).await;
                            let elapsed = t_start.elapsed().as_secs_f32();

                            if success {
                                log::info!("Upload SUCCESS: {} ({:.2}s)", file_task.path, elapsed);
                                let mut pc_lock = p_count.lock().await;
                                *pc_lock += 1;
                            } else {
                                log::warn!("Upload FAILED: {} ({:.2}s). Adding to retry queue.", file_task.path, elapsed);
                                append_retry(&retry_path_clone, &file_task);
                            }

                            let new_pc = *p_count.lock().await;
                            let tq_now = *t_queued.lock().await;
                            let active_now = {
                                let mut a = active.lock().await;
                                *a -= 1;
                                *a
                            };

                            // Transition to idle when all uploads complete
                            if new_pc >= tq_now && active_now == 0 {
                                log::info!("All {} file(s) processed. Idle.", tq_now);
                                sm.write_state(AppState {
                                    queue_size: 0,
                                    total_queued: tq_now,
                                    processed_count: new_pc,
                                    current_file: None,
                                    status: "idle".to_string(),
                                    progress: 100,
                                    timestamp: 0.0,
                                });
                                notifications::send(
                                    "Upload Complete",
                                    &format!("Processed {} file(s).", new_pc),
                                    Some(100),
                                );
                            } else {
                                let prog = if tq_now > 0 { ((new_pc as f32 / tq_now as f32) * 100.0) as u8 } else { 0 };
                                sm.write_state(AppState {
                                    queue_size: tq_now.saturating_sub(new_pc),
                                    total_queued: tq_now,
                                    processed_count: new_pc,
                                    current_file: None,
                                    status: "uploading".to_string(),
                                    progress: prog,
                                    timestamp: 0.0,
                                });
                            }
                        }
                        None => {
                            log::debug!("Worker {} channel closed, exiting.", i);
                            break;
                        }
                    }
                }
            });
        }

        // Re-queue persisted retries after a short delay
        let sender_clone = qm.sender.clone();
        let tq_clone = total_queued.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            for task in loaded_retries {
                log::info!("Re-queuing from retry: {}", task.path);
                {
                    let mut tq = tq_clone.lock().await;
                    *tq += 1;
                }
                let _ = sender_clone.send(task).await;
            }

            // Periodic retry worker (every 60s, matching Python)
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                // Retries are appended atomically; nothing more to do here
                // unless we implement a full retry_queue in-memory structure
            }
        });

        qm
    }

    /// Add a file task to the upload queue.
    pub async fn add_to_queue(&self, task: FileTask) {
        log::debug!("Queuing: {}", task.path);
        {
            let mut tq = self.total_queued.lock().await;
            *tq += 1;
            log::debug!("Queue depth: {}", *tq);
        }
        if let Err(e) = self.sender.send(task).await {
            log::error!("Failed to send task to queue: {}", e);
        }
    }
}

/// Upload a file and add it to the appropriate album.
async fn handle_upload(api: &ImmichApiClient, task: &FileTask) -> bool {
    let asset_id = api.upload_asset(&task.path, &task.checksum).await;

    let asset_id = match asset_id {
        None => return false,
        Some(ref id) if id == "DUPLICATE" => {
            log::info!("Asset already on server: {}", task.path);
            return true;
        }
        Some(id) => id,
    };

    // Determine album name (fall back to parent directory name like Python does)
    let album_name = match (&task.album_name, &task.album_id) {
        (Some(name), _) if !name.is_empty() && name != "Default (Folder Name)" => name.clone(),
        _ => {
            std::path::Path::new(&task.path)
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Mimick".to_string())
        }
    };

    log::info!("Adding '{}' to album '{}'", task.path, album_name);

    // Use explicit album_id or resolve via get_or_create
    let final_album_id = if let Some(ref id) = task.album_id {
        if !id.is_empty() {
            Some(id.clone())
        } else {
            api.get_or_create_album(&album_name).await
        }
    } else {
        api.get_or_create_album(&album_name).await
    };

    if let Some(album_id) = final_album_id {
        api.add_assets_to_album(&album_id, &[asset_id]).await;
    } else {
        log::warn!("Could not resolve album '{}'. Asset uploaded but not added to album.", album_name);
    }

    true
}

/// Append a failed task to the retry JSON file (deduplicated, atomic write).
fn append_retry(path: &PathBuf, task: &FileTask) {
    let mut tasks = load_retries(path);
    // Deduplicate: don't add the same path twice
    if !tasks.iter().any(|t| t.path == task.path) {
        tasks.push(task.clone());
        log::debug!("Retry queue: {} item(s)", tasks.len());
        save_retries(path, &tasks);
    } else {
        log::debug!("Retry already queued, skipping: {}", task.path);
    }
}

fn save_retries(path: &PathBuf, tasks: &[FileTask]) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(content) = serde_json::to_string(tasks) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, content).is_ok() {
            if let Err(e) = fs::rename(&tmp, path) {
                log::warn!("Failed to save retries: {}", e);
            } else {
                log::debug!("Retries saved: {} item(s)", tasks.len());
            }
        }
    }
}

fn load_retries(path: &PathBuf) -> Vec<FileTask> {
    if !path.exists() { return Vec::new(); }
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::error!("Failed to load retries: {}", e);
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_filetask_serialization() {
        let task = FileTask {
            path: "/a/b.jpg".to_string(),
            checksum: "sha123".to_string(),
            album_id: Some("id1".to_string()),
            album_name: Some("Album".to_string()),
        };
        let js = serde_json::to_string(&task).unwrap();
        assert!(js.contains("sha123"));
        
        let deserialized: FileTask = serde_json::from_str(&js).unwrap();
        assert_eq!(deserialized.path, "/a/b.jpg");
        assert_eq!(deserialized.album_id.unwrap(), "id1");
    }

    #[test]
    fn test_retry_queue_logic() {
        let dir = tempdir().unwrap();
        let retry_path = dir.path().join("retries.json");

        let task1 = FileTask {
            path: "/a/1.jpg".to_string(),
            checksum: "hash1".to_string(),
            album_id: None,
            album_name: None,
        };

        // Initially empty
        let loaded = load_retries(&retry_path);
        assert_eq!(loaded.len(), 0);

        // Append one
        append_retry(&retry_path, &task1);
        let loaded = load_retries(&retry_path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].path, "/a/1.jpg");

        // Append same one again (deduplication should kick in)
        append_retry(&retry_path, &task1);
        let loaded = load_retries(&retry_path);
        assert_eq!(loaded.len(), 1);

        // Append different
        let task2 = FileTask {
            path: "/a/2.jpg".to_string(),
            checksum: "hash2".to_string(),
            album_id: None,
            album_name: None,
        };
        append_retry(&retry_path, &task2);
        let loaded = load_retries(&retry_path);
        assert_eq!(loaded.len(), 2);
    }
}
