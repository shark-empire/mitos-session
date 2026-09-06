use crate::config::{IdleSettings, LockSettings};
use crate::idle::IdlePolicy;
use crate::lock::LockPolicy;

/// Bundles the lock and idle policies together, since "how long until
/// idle locks the screen" spans both `[lock]` and `[idle]` in config,
/// and every call site that needs one basically needs the other too.
#[derive(Debug, Clone, Copy)]
pub struct LockChainPolicy {
    pub lock: LockPolicy,
    pub idle: IdlePolicy,
}

impl LockChainPolicy {
    pub fn new(lock: &LockSettings, idle: &IdleSettings) -> Self {
        Self {
            lock: LockPolicy::from(lock),
            idle: IdlePolicy::from(idle),
        }
    }
}
