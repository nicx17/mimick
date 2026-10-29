use std::sync::Arc;

use crate::notifications;
use crate::state_manager::AppState;

#[derive(Debug, Default)]
pub(super) struct BatchNotifyState {
    /// Active status indicating whether a batch notifications window is open.
    active: bool,
    /// True if a notification summary sweep is already planned.
    notify_scheduled: bool,
    /// Unique identifier of the current batch.
    current_batch_id: u64,
    /// Last unique identifier that triggered desktop notification.
    last_notified_batch_id: u64,
    /// Processed count recorded at beginning of batch.
    start_processed: usize,
    /// Failed count recorded at beginning of batch.
    start_failed: usize,
}

pub(super) fn check_batch_notification(batch_state: &mut BatchNotifyState) -> Option<u64> {
    if batch_state.active
        && batch_state.current_batch_id != batch_state.last_notified_batch_id
        && !batch_state.notify_scheduled
    {
        batch_state.notify_scheduled = true;
        Some(batch_state.current_batch_id)
    } else {
        None
    }
}

/// Spawn a debounced batch-notification task.
pub(super) fn schedule_batch_notification(
    state_ref: Arc<parking_lot::Mutex<AppState>>,
    batch_notify_ref: Arc<parking_lot::Mutex<BatchNotifyState>>,
    batch_id: u64,
) {
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        let summary = batch_notification_summary(&state_ref, &batch_notify_ref, batch_id);
        if let Some((succeeded, failed)) = summary {
            notifications::send_sync_summary(succeeded, failed);
        }
    });
}

fn batch_notification_summary(
    state_ref: &Arc<parking_lot::Mutex<AppState>>,
    batch_notify_ref: &Arc<parking_lot::Mutex<BatchNotifyState>>,
    batch_id: u64,
) -> Option<(usize, usize)> {
    let s = state_ref.lock();
    let mut batch_state = batch_notify_ref.lock();
    if !batch_is_ready_to_notify(&s, &batch_state, batch_id) {
        clear_stale_batch_schedule(&mut batch_state, batch_id);
        return None;
    }
    let succeeded = s
        .processed_count
        .saturating_sub(batch_state.start_processed);
    let failed = s.failed_count.saturating_sub(batch_state.start_failed);
    batch_state.last_notified_batch_id = batch_id;
    batch_state.notify_scheduled = false;
    batch_state.active = false;
    Some((succeeded, failed))
}

fn batch_is_ready_to_notify(
    state: &AppState,
    batch_state: &BatchNotifyState,
    batch_id: u64,
) -> bool {
    let total_handled = state.processed_count + state.failed_count;
    let queue_idle = total_handled >= state.total_queued && state.active_workers == 0;
    batch_state.active
        && batch_state.notify_scheduled
        && batch_state.current_batch_id == batch_id
        && batch_state.current_batch_id != batch_state.last_notified_batch_id
        && queue_idle
}

fn clear_stale_batch_schedule(batch_state: &mut BatchNotifyState, batch_id: u64) {
    if batch_state.current_batch_id == batch_id {
        batch_state.notify_scheduled = false;
    }
}

pub(super) fn activate_batch_if_needed(batch_state: &mut BatchNotifyState, state: &AppState) {
    if batch_state.active {
        return;
    }

    // Start a fresh notification batch when work is (re)introduced.
    batch_state.active = true;
    batch_state.notify_scheduled = false;
    batch_state.current_batch_id = batch_state.current_batch_id.saturating_add(1);
    batch_state.start_processed = state.processed_count;
    batch_state.start_failed = state.failed_count;
}
