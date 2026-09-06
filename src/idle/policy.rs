use crate::config::IdleSettings;
use std::time::Duration;

/// The three idle thresholds, converted from the on-disk `u64` seconds
/// into real `Duration`s once at startup (or config reload) rather
/// than on every tick.
#[derive(Debug, Clone, Copy)]
pub struct IdlePolicy {
    pub dim_after: Duration,
    pub lock_after: Duration,
    pub suspend_after: Duration,
}

impl From<&IdleSettings> for IdlePolicy {
    fn from(s: &IdleSettings) -> Self {
        Self {
            dim_after: Duration::from_secs(s.dim_after_secs),
            lock_after: Duration::from_secs(s.lock_after_secs),
            suspend_after: Duration::from_secs(s.suspend_after_secs),
        }
    }
}

impl IdlePolicy {
    /// Which stage `idle_for` falls into. A threshold of zero disables
    /// that stage (it's simply never reached), so setting
    /// `suspend_after_secs = 0` in config gives you "dim and lock, but
    /// never auto-suspend."
    pub fn stage_for(&self, idle_for: Duration) -> IdleStage {
        let reaches = |threshold: Duration| threshold > Duration::ZERO && idle_for >= threshold;
        if reaches(self.suspend_after) {
            IdleStage::SuspendRequested
        } else if reaches(self.lock_after) {
            IdleStage::LockRequested
        } else if reaches(self.dim_after) {
            IdleStage::Dimmed
        } else {
            IdleStage::Active
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdleStage {
    Active,
    Dimmed,
    LockRequested,
    SuspendRequested,
}
