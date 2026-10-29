use super::{TransferDirection, TransferSnapshot};

impl TransferSnapshot {
    /// Reset active metrics back to zero.
    pub fn reset_runtime(&mut self) {
        self.active = false;
        self.direction = TransferDirection::Upload;
        self.current_bytes = 0;
        self.total_bytes = None;
        self.session_total_bytes = 0;
        self.session_transferred_bytes = 0;
        self.completed_total_bytes = 0;
        self.completed_transferred_bytes = 0;
        self.instant_bps = 0.0;
        self.session_avg_bps = 0.0;
        self.session_started_at = None;
        self.session_bytes_done = 0;
        self.active_item_label = None;
        self.active_route = None;
        self.last_tick_at = None;
        self.last_tick_bytes = 0;
        self.active_uploads = 0;
        self.active_downloads = 0;
        self.active_item_bytes.clear();
        self.active_item_totals.clear();
    }

    /// Check if target transfer direction should be switched.
    fn should_switch_to(&self, direction: &TransferDirection) -> bool {
        if !self.active {
            return true;
        }
        match (&self.direction, direction) {
            (TransferDirection::Upload, TransferDirection::Download) => self.active_uploads == 0,
            (_, TransferDirection::Upload) => true,
            _ => self.direction == *direction,
        }
    }

    /// Ensure an active transfer session group is initialized.
    fn ensure_session(&mut self, direction: TransferDirection, route: Option<String>) {
        if self.should_switch_to(&direction) {
            self.active = true;
            self.direction = direction;
            self.current_bytes = 0;
            self.total_bytes = Some(0);
            self.session_total_bytes = 0;
            self.session_transferred_bytes = 0;
            self.completed_total_bytes = 0;
            self.completed_transferred_bytes = 0;
            self.instant_bps = 0.0;
            self.session_avg_bps = 0.0;
            self.session_started_at = None;
            self.session_bytes_done = 0;
            self.active_item_label = None;
            self.active_route = route;
            self.last_tick_at = None;
            self.last_tick_bytes = 0;
            self.active_item_bytes.clear();
            self.active_item_totals.clear();
        } else if route.is_some() {
            self.active_route = route;
        }
    }

    /// Add an active item structure into the ongoing transfer snapshot.
    pub fn register_item(
        &mut self,
        direction: TransferDirection,
        item_id: impl Into<String>,
        total_bytes: Option<u64>,
        item_label: Option<String>,
        route: Option<String>,
    ) {
        self.ensure_session(direction.clone(), route.clone());
        let item_id = item_id.into();
        let previous_total = self
            .active_item_totals
            .insert(item_id.clone(), total_bytes.unwrap_or(0));
        if previous_total.is_none() {
            match direction {
                TransferDirection::Upload => self.active_uploads += 1,
                TransferDirection::Download => self.active_downloads += 1,
            }
        }
        self.active_item_bytes.entry(item_id).or_insert(0);
        let active_totals: u64 = self.active_item_totals.values().copied().sum();
        self.session_total_bytes = self.completed_total_bytes + active_totals;
        self.total_bytes = Some(self.session_total_bytes);
        if self.active_item_label.is_none() {
            self.active_item_label = item_label;
        }
    }

    /// Start tracking a group of transfer operations.
    pub fn begin_group(
        &mut self,
        direction: TransferDirection,
        item_label: Option<String>,
        route: Option<String>,
    ) {
        self.ensure_session(direction, route.clone());
        if self.active_item_label.is_none() {
            self.active_item_label = item_label;
        }
        if route.is_some() {
            self.active_route = route;
        }
    }

    /// Update total sizes for a specific active item in the transfer.
    pub fn update_item_total(&mut self, item_id: &str, total_bytes: u64) {
        self.active_item_totals
            .insert(item_id.to_string(), total_bytes);
        let active_totals: u64 = self.active_item_totals.values().copied().sum();
        self.session_total_bytes = self.completed_total_bytes + active_totals;
        self.total_bytes = Some(self.session_total_bytes);
    }

    /// Update bytes completed so far for a specific active item.
    pub fn update_item_bytes(
        &mut self,
        direction: TransferDirection,
        item_id: &str,
        bytes_done: u64,
        route: Option<String>,
    ) {
        if !self.active || self.direction != direction {
            return;
        }

        self.active_item_bytes
            .insert(item_id.to_string(), bytes_done);
        let active_bytes: u64 = self.active_item_bytes.values().copied().sum();
        self.session_transferred_bytes = self.completed_transferred_bytes + active_bytes;
        self.current_bytes = self.session_transferred_bytes;
        self.session_bytes_done = self.session_transferred_bytes;
        self.total_bytes = Some(self.session_total_bytes);

        self.update_transfer_speed(super::unix_timestamp_now(), route);
    }

    fn update_transfer_speed(&mut self, now: f64, route: Option<String>) {
        if self.session_started_at.is_none() {
            self.session_started_at = Some(now);
            self.last_tick_at = Some(now);
            self.last_tick_bytes = self.session_transferred_bytes;
            self.instant_bps = 0.0;
            self.session_avg_bps = 0.0;
            if route.is_some() {
                self.active_route = route;
            }
            return;
        }
        let mut do_update_speed = false;
        let mut previous_bytes = 0;

        let previous_tick = self.last_tick_at.unwrap_or(now);
        let elapsed = now - previous_tick;
        if elapsed >= 0.25 {
            do_update_speed = true;
            previous_bytes = self.last_tick_bytes;
        }

        if let Some(started) = self.session_started_at {
            self.session_avg_bps =
                self.session_transferred_bytes as f64 / (now - started).max(0.001);
        }

        if do_update_speed {
            let delta = self
                .session_transferred_bytes
                .saturating_sub(previous_bytes);
            self.instant_bps = delta as f64 / elapsed;
            self.last_tick_at = Some(now);
            self.last_tick_bytes = self.session_transferred_bytes;
        }

        if route.is_some() {
            self.active_route = route;
        }
    }

    /// Finish tracking a specific item, returning true if the session is empty.
    pub fn finish_item(
        &mut self,
        direction: TransferDirection,
        item_id: &str,
        route: Option<String>,
    ) -> bool {
        let completed_avg = self.session_avg_bps;
        let item_final_bytes = self.active_item_bytes.remove(item_id).unwrap_or(0);
        let item_final_total = self.active_item_totals.remove(item_id).unwrap_or(0);

        self.completed_transferred_bytes += item_final_bytes;
        self.completed_total_bytes += item_final_total;

        let active_bytes: u64 = self.active_item_bytes.values().copied().sum();
        let active_total: u64 = self.active_item_totals.values().copied().sum();

        self.session_transferred_bytes = self.completed_transferred_bytes + active_bytes;
        self.session_total_bytes = self.completed_total_bytes + active_total;

        self.current_bytes = self.session_transferred_bytes;
        self.total_bytes = Some(self.session_total_bytes);

        match direction {
            TransferDirection::Upload => {
                self.active_uploads = self.active_uploads.saturating_sub(1)
            }
            TransferDirection::Download => {
                self.active_downloads = self.active_downloads.saturating_sub(1)
            }
        }

        match direction {
            TransferDirection::Upload => self.last_upload_avg_bps = completed_avg,
            TransferDirection::Download => self.last_download_avg_bps = completed_avg,
        }

        if route.is_some() {
            self.active_route = route;
        }

        if self.active_uploads == 0 && self.active_downloads == 0 {
            self.reset_runtime();
            return true;
        } else if self.active_uploads == 0 && self.direction == TransferDirection::Upload {
            self.direction = TransferDirection::Download;
            self.current_bytes = 0;
            self.total_bytes = Some(self.session_total_bytes);
            self.instant_bps = 0.0;
            self.session_avg_bps = 0.0;
            self.session_started_at = None;
            self.session_bytes_done = self.session_transferred_bytes;
            self.active_item_label = None;
            self.last_tick_at = None;
            self.last_tick_bytes = 0;
        }
        false
    }
}
