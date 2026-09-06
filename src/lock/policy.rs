use crate::config::LockSettings;
use std::time::Duration;

/// Runtime lock policy, built once from config.
#[derive(Debug, Clone, Copy)]
pub struct LockPolicy {
    pub enabled: bool,
    pub lock_on_idle: bool,
    pub lock_on_suspend: bool,
    pub grace_period: Duration,
}

impl From<&LockSettings> for LockPolicy {
    fn from(s: &LockSettings) -> Self {
        Self {
            enabled: s.enabled,
            lock_on_idle: s.lock_on_idle,
            lock_on_suspend: s.lock_on_suspend,
            grace_period: Duration::from_secs(s.grace_period_secs),
        }
    }
}
