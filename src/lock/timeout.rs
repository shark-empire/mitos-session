use crate::session::SessionId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The "wait, then do something" timers the lock subsystem needs that
/// don't belong in `idle` (which only measures raw input, not
/// lock-specific grace/backoff periods): the grace period between an
/// idle-triggered lock request and actually engaging the lock, and the
/// countdown on a lockout after too many bad passwords.
#[derive(Debug, Default)]
pub struct LockTimeouts {
    grace_expiry: HashMap<SessionId, Instant>,
    lockout_expiry: HashMap<SessionId, Instant>,
}

impl LockTimeouts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_grace(&mut self, session: SessionId, now: Instant, grace: Duration) {
        self.grace_expiry.insert(session, now + grace);
    }

    pub fn cancel_grace(&mut self, session: SessionId) {
        self.grace_expiry.remove(&session);
    }

    /// Sessions whose grace period has elapsed as of `now`. The caller
    /// should actually engage the lock for each of these; they're
    /// removed from tracking as they're returned.
    pub fn expired_grace(&mut self, now: Instant) -> Vec<SessionId> {
        Self::drain_expired(&mut self.grace_expiry, now)
    }

    pub fn start_lockout(&mut self, session: SessionId, now: Instant, lockout: Duration) {
        self.lockout_expiry.insert(session, now + lockout);
    }

    pub fn expired_lockouts(&mut self, now: Instant) -> Vec<SessionId> {
        Self::drain_expired(&mut self.lockout_expiry, now)
    }

    fn drain_expired(map: &mut HashMap<SessionId, Instant>, now: Instant) -> Vec<SessionId> {
        let expired: Vec<SessionId> = map
            .iter()
            .filter(|&(_, &t)| t <= now)
            .map(|(&id, _)| id)
            .collect();
        for id in &expired {
            map.remove(id);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grace_period_expires_once() {
        let mut timeouts = LockTimeouts::new();
        let t0 = Instant::now();
        timeouts.start_grace(1, t0, Duration::from_secs(5));

        assert!(timeouts
            .expired_grace(t0 + Duration::from_secs(3))
            .is_empty());
        assert_eq!(timeouts.expired_grace(t0 + Duration::from_secs(6)), vec![1]);
        // Already drained -- doesn't fire again.
        assert!(timeouts
            .expired_grace(t0 + Duration::from_secs(10))
            .is_empty());
    }
}
