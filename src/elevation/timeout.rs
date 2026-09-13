use super::manager::ElevationRequestId;
use crate::session::SessionId;
use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// The two "wait, then do something" timers elevation needs: how long
/// an open prompt waits for an answer before it's treated as timed
/// out, and how long a session stays locked out after too many wrong
/// guesses. Mirrors `lock::LockTimeouts` closely -- same shape, kept
/// separate because elevation prompts and screen-lock attempts are
/// different flows with (usually) different durations, and because a
/// prompt here is keyed by request, not by session.
#[derive(Debug, Default)]
pub struct ElevationTimeouts {
    prompt_expiry: HashMap<ElevationRequestId, Instant>,
    lockout_expiry: HashMap<SessionId, Instant>,
}

impl ElevationTimeouts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_prompt(&mut self, id: ElevationRequestId, now: Instant, timeout: Duration) {
        self.prompt_expiry.insert(id, now + timeout);
    }

    /// Stop tracking `id`'s prompt timeout -- called once it's
    /// resolved by any means (answered, cancelled, or abandoned) so
    /// the timeout sweep doesn't try to expire it a second time.
    pub fn cancel_prompt(&mut self, id: ElevationRequestId) {
        self.prompt_expiry.remove(&id);
    }

    /// IDs whose prompt window has elapsed as of `now`, removed from
    /// tracking as they're returned.
    pub fn expired_prompts(&mut self, now: Instant) -> Vec<ElevationRequestId> {
        Self::drain_expired(&mut self.prompt_expiry, now)
    }

    pub fn start_lockout(&mut self, session: SessionId, now: Instant, lockout: Duration) {
        self.lockout_expiry.insert(session, now + lockout);
    }

    /// Sessions whose lockout has elapsed as of `now`, removed from
    /// tracking as they're returned.
    pub fn expired_lockouts(&mut self, now: Instant) -> Vec<SessionId> {
        Self::drain_expired(&mut self.lockout_expiry, now)
    }

    fn drain_expired<K: Hash + Eq + Copy>(map: &mut HashMap<K, Instant>, now: Instant) -> Vec<K> {
        let expired: Vec<K> = map
            .iter()
            .filter(|&(_, &t)| t <= now)
            .map(|(&k, _)| k)
            .collect();
        for k in &expired {
            map.remove(k);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_expires_only_once_its_timeout_has_elapsed() {
        let mut t = ElevationTimeouts::new();
        let now = Instant::now();
        t.start_prompt(1, now, Duration::from_secs(10));

        assert!(t.expired_prompts(now + Duration::from_secs(5)).is_empty());
        assert_eq!(t.expired_prompts(now + Duration::from_secs(10)), vec![1]);
        // Only reported once -- draining removes it.
        assert!(t.expired_prompts(now + Duration::from_secs(20)).is_empty());
    }

    #[test]
    fn cancelling_a_prompt_stops_it_from_expiring() {
        let mut t = ElevationTimeouts::new();
        let now = Instant::now();
        t.start_prompt(1, now, Duration::from_secs(10));
        t.cancel_prompt(1);
        assert!(t.expired_prompts(now + Duration::from_secs(20)).is_empty());
    }

    #[test]
    fn lockouts_and_prompts_dont_interfere_with_each_other() {
        let mut t = ElevationTimeouts::new();
        let now = Instant::now();
        t.start_prompt(1, now, Duration::from_secs(10));
        t.start_lockout(1, now, Duration::from_secs(10)); // session id 1, unrelated to request id 1
        assert_eq!(t.expired_prompts(now + Duration::from_secs(10)), vec![1]);
        assert_eq!(t.expired_lockouts(now + Duration::from_secs(10)), vec![1]);
    }
}
