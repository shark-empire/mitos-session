use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Per-seat "when did we last see input" bookkeeping. Activity reports
/// arrive as coalesced pings from the compositor over IPC
/// (`ipc::messages::Request::ReportActivity`) -- this module never
/// touches raw input devices itself.
#[derive(Debug, Default)]
pub struct IdleTracker {
    last_activity: HashMap<String, Instant>,
}

impl IdleTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_activity(&mut self, seat_id: &str, now: Instant) {
        self.last_activity.insert(seat_id.to_string(), now);
    }

    pub fn idle_for(&self, seat_id: &str, now: Instant) -> Duration {
        self.last_activity
            .get(seat_id)
            .map(|&last| now.saturating_duration_since(last))
            .unwrap_or_default()
    }

    pub fn seats(&self) -> impl Iterator<Item = &str> {
        self.last_activity.keys().map(String::as_str)
    }
}
