use super::policy::{IdlePolicy, IdleStage};
use super::tracker::IdleTracker;
use std::collections::HashMap;
use std::time::Instant;

/// Combines `IdleTracker`'s raw timestamps with an `IdlePolicy` to
/// produce stage-change events. Call `tick` from a periodic (1s)
/// calloop timer rather than scheduling a fresh one-shot timer per
/// threshold per seat -- simpler, and cheap enough at 1Hz across a
/// handful of seats that it isn't worth the extra bookkeeping.
#[derive(Default)]
pub struct IdleDetector {
    tracker: IdleTracker,
    stage: HashMap<String, IdleStage>,
}

impl IdleDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_activity(&mut self, seat_id: &str, now: Instant) {
        self.tracker.record_activity(seat_id, now);
        self.stage.insert(seat_id.to_string(), IdleStage::Active);
    }

    /// Recompute every tracked seat's stage against `policy`. Returns
    /// `(seat_id, new_stage)` for every seat whose stage just changed,
    /// so the caller only reacts to transitions rather than polling
    /// steady state itself.
    pub fn tick(&mut self, now: Instant, policy: &IdlePolicy) -> Vec<(String, IdleStage)> {
        let mut changes = Vec::new();
        let seat_ids: Vec<String> = self.tracker.seats().map(str::to_string).collect();
        for seat_id in seat_ids {
            let idle_for = self.tracker.idle_for(&seat_id, now);
            let new_stage = policy.stage_for(idle_for);
            let entry = self.stage.entry(seat_id.clone()).or_insert(IdleStage::Active);
            if *entry != new_stage {
                *entry = new_stage;
                changes.push((seat_id, new_stage));
            }
        }
        changes
    }

    pub fn stage(&self, seat_id: &str) -> IdleStage {
        self.stage.get(seat_id).copied().unwrap_or(IdleStage::Active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn policy() -> IdlePolicy {
        IdlePolicy {
            dim_after: Duration::from_secs(10),
            lock_after: Duration::from_secs(20),
            suspend_after: Duration::from_secs(30),
        }
    }

    #[test]
    fn crosses_thresholds_in_order() {
        let policy = policy();
        let mut detector = IdleDetector::new();
        let t0 = Instant::now();
        detector.record_activity("seat0", t0);

        assert!(detector.tick(t0 + Duration::from_secs(5), &policy).is_empty());

        let changes = detector.tick(t0 + Duration::from_secs(11), &policy);
        assert_eq!(changes, vec![("seat0".to_string(), IdleStage::Dimmed)]);

        let changes = detector.tick(t0 + Duration::from_secs(21), &policy);
        assert_eq!(changes, vec![("seat0".to_string(), IdleStage::LockRequested)]);
    }

    #[test]
    fn activity_resets_to_active() {
        let policy = policy();
        let mut detector = IdleDetector::new();
        let t0 = Instant::now();
        detector.record_activity("seat0", t0);
        detector.tick(t0 + Duration::from_secs(15), &policy);
        assert_eq!(detector.stage("seat0"), IdleStage::Dimmed);

        detector.record_activity("seat0", t0 + Duration::from_secs(16));
        assert_eq!(detector.stage("seat0"), IdleStage::Active);
    }
}
