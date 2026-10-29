use super::*;
use crate::state_manager::AppState;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::mpsc;

type TestQueueManagerParts = (
    QueueManager,
    mpsc::Receiver<FileTask>,
    Arc<Mutex<AppState>>,
    Arc<Mutex<Vec<FileTask>>>,
    Arc<Mutex<HashSet<String>>>,
);

fn test_queue_manager(buffer: usize) -> TestQueueManagerParts {
    let (tx, rx) = mpsc::channel(buffer);
    let shared_state = Arc::new(Mutex::new(AppState::default()));
    let retry_list = Arc::new(Mutex::new(Vec::<FileTask>::new()));
    let pending_paths = Arc::new(Mutex::new(HashSet::<String>::new()));
    let retry_path = tempdir().unwrap().path().join("retries.json");

    (
        QueueManager {
            sender: tx,
            shared_state: shared_state.clone(),
            retry_list: retry_list.clone(),
            pending_paths: pending_paths.clone(),
            retry_path,
            policy: Arc::new(Mutex::new(EnvironmentPolicy {
                pause_on_metered_network: false,
                pause_on_battery_power: false,
                quiet_hours_start: None,
                quiet_hours_end: None,
            })),
            worker_limit: Arc::new(AtomicUsize::new(1)),
            limit_changed: Arc::new(Notify::new()),
            batch_notify_state: Arc::new(Mutex::new(BatchNotifyState::default())),
            accepting_new: Arc::new(AtomicBool::new(true)),
            shutdown_token: CancellationToken::new(),
        },
        rx,
        shared_state,
        retry_list,
        pending_paths,
    )
}

#[test]
fn test_filetask_serialization() {
    let task = FileTask {
        path: "/a/b.jpg".to_string(),
        watch_path: "/a".to_string(),
        checksum: "sha123".to_string(),
        album_id: Some("id1".to_string()),
        album_name: Some("Album".to_string()),
        reassociate_only: false,
        skip_album: false,
        sidecar_path: None,
    };
    let js = serde_json::to_string(&task).unwrap();
    assert!(js.contains("sha123"));

    let deserialized: FileTask = serde_json::from_str(&js).unwrap();
    assert_eq!(deserialized.path, "/a/b.jpg");
    assert_eq!(deserialized.album_id.unwrap(), "id1");
}

#[test]
fn test_filetask_skip_album_defaults_false_for_legacy_payloads() {
    // Retry-queue entries written before `skip_album` existed must still
    // deserialise — they keep the old watch-folder behaviour.
    let legacy_json = r#"{
        "path": "/a/b.jpg",
        "watch_path": "/a",
        "checksum": "sha123",
        "album_id": null,
        "album_name": null,
        "reassociate_only": false
    }"#;
    let task: FileTask = serde_json::from_str(legacy_json).unwrap();
    assert!(
        !task.skip_album,
        "legacy entries inherit album-creation behaviour"
    );
    assert!(
        task.sidecar_path.is_none(),
        "legacy entries default to no sidecar"
    );
}

#[test]
fn test_filetask_skip_album_round_trips() {
    let task = FileTask {
        path: "/x.jpg".into(),
        watch_path: String::new(),
        checksum: "sha".into(),
        album_id: None,
        album_name: None,
        reassociate_only: false,
        skip_album: true,
        sidecar_path: None,
    };
    let js = serde_json::to_string(&task).unwrap();
    let restored: FileTask = serde_json::from_str(&js).unwrap();
    assert!(restored.skip_album);
}

#[test]
fn test_retry_persistence() {
    let dir = tempdir().unwrap();
    let retry_path = dir.path().join("retries.json");

    let task = FileTask {
        path: "/a/1.jpg".to_string(),
        watch_path: "/a".to_string(),
        checksum: "hash1".to_string(),
        album_id: None,
        album_name: None,
        reassociate_only: false,
        skip_album: false,
        sidecar_path: None,
    };

    let tasks = vec![task];
    save_retries(&retry_path, &tasks);
    let loaded = load_retries(&retry_path);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].path, "/a/1.jpg");
}

#[tokio::test]
async fn test_add_to_queue_rejects_duplicate_pending_path() {
    let (qm, mut rx, shared_state, _retry_list, pending_paths) = test_queue_manager(4);
    let task = FileTask {
        path: "/a/1.jpg".to_string(),
        watch_path: "/a".to_string(),
        checksum: "hash1".to_string(),
        album_id: None,
        album_name: Some("Album".into()),
        reassociate_only: false,
        skip_album: false,
        sidecar_path: None,
    };

    assert!(qm.add_to_queue(task.clone()).await);
    assert!(!qm.add_to_queue(task.clone()).await);

    let queued = rx.recv().await.unwrap();
    assert_eq!(queued.path, task.path);
    assert!(pending_paths.lock().contains("/a/1.jpg"));
    assert_eq!(shared_state.lock().total_queued, 1);
}

#[tokio::test]
async fn test_retry_failed_path_requeues_and_updates_state() {
    let (qm, mut rx, shared_state, retry_list, pending_paths) = test_queue_manager(4);
    let task = FileTask {
        path: "/a/failed.jpg".to_string(),
        watch_path: "/a".to_string(),
        checksum: "hash1".to_string(),
        album_id: None,
        album_name: None,
        reassociate_only: false,
        skip_album: false,
        sidecar_path: None,
    };

    retry_list.lock().push(task.clone());
    pending_paths.lock().insert(task.path.clone());
    shared_state.lock().failed_count = 1;

    assert!(qm.retry_failed_path(&task.path).await);

    let requeued = rx.recv().await.unwrap();
    assert_eq!(requeued.path, task.path);
    assert!(retry_list.lock().is_empty());

    let state = shared_state.lock();
    assert_eq!(state.failed_count, 0);
    assert_eq!(state.total_queued, 1);
    assert_eq!(state.recent_events[0].status, "pending");
    assert_eq!(state.recent_events[0].attempts, 2);
}

#[test]
fn test_unix_timestamp_now_is_non_zero() {
    assert!(unix_timestamp_now() > 0.0);
}

#[test]
fn test_clear_failed_removes_retry_entries_and_pending_paths() {
    let (qm, _rx, shared_state, retry_list, pending_paths) = test_queue_manager(4);
    let task = FileTask {
        path: "/a/failed.jpg".to_string(),
        watch_path: "/a".to_string(),
        checksum: "hash1".to_string(),
        album_id: None,
        album_name: None,
        reassociate_only: false,
        skip_album: false,
        sidecar_path: None,
    };

    retry_list.lock().push(task.clone());
    pending_paths.lock().insert(task.path.clone());
    shared_state.lock().failed_count = 1;

    assert_eq!(qm.clear_failed(), 1);
    assert!(retry_list.lock().is_empty());
    assert!(!pending_paths.lock().contains(&task.path));

    let state = shared_state.lock();
    assert_eq!(state.failed_count, 0);
    assert_eq!(state.recent_events[0].status, "cleared");
}
