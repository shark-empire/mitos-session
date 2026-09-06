use super::inhibitor::{InhibitMode, InhibitWhat, InhibitorRegistry};
use super::policy::LockPolicy;
use super::state::LockState;
use super::timeout::LockTimeouts;
use crate::authentication::{check, AuthOutcome, AuthPolicy, AuthRequest, Authenticator};
use crate::errors::{Result, SessionError};
use crate::session::SessionId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Why a lock was engaged -- carried in `ipc::messages::Event::ShowLockScreen`
/// so the compositor can show a different message ("Locked" vs "Session
/// suspended") without mitos-session needing to know anything about
/// presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockReason {
    Manual,
    Idle,
    Suspend,
}

#[derive(Debug, Default)]
struct SessionLock {
    state: LockState,
    attempts: u32,
}

/// Owns the lock state machine for every session, the shared inhibitor
/// registry (inhibitors are global, not per-session), and the
/// grace/lockout timers. Delegates the actual credential check to
/// `authentication::check` -- this module only ever sees the outcome.
#[derive(Default)]
pub struct LockManager {
    locks: HashMap<SessionId, SessionLock>,
    pub inhibitors: InhibitorRegistry,
    pub timeouts: LockTimeouts,
}

impl LockManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_locked(&self, session: SessionId) -> bool {
        matches!(
            self.locks.get(&session).map(|l| l.state),
            Some(LockState::Locked) | Some(LockState::LockedOut)
        )
    }

    /// Engage the lock for `session` right now, unless a `Block`
    /// inhibitor on `InhibitWhat::Lock` is currently held (e.g. a
    /// presentation app asked to keep the screen up).
    pub fn lock(&mut self, session: SessionId, policy: &LockPolicy) -> Result<()> {
        if !policy.enabled {
            return Err(SessionError::Unsupported(
                "locking is disabled in config".into(),
            ));
        }
        if self.inhibitors.blocks(InhibitWhat::Lock) {
            return Err(SessionError::PermissionDenied(
                "an application is inhibiting the lock screen".into(),
            ));
        }
        self.locks.entry(session).or_default().state = LockState::Locked;
        Ok(())
    }

    pub fn unlock(&mut self, session: SessionId) {
        if let Some(lock) = self.locks.get_mut(&session) {
            lock.state = LockState::Unlocked;
            lock.attempts = 0;
        }
        self.timeouts.cancel_grace(session);
    }

    /// Run `request`'s credential through `authenticator`, update this
    /// session's attempt count/lockout state, and return the outcome
    /// to relay back to the compositor as `Event::AuthFeedback`.
    pub fn attempt_unlock(
        &mut self,
        authenticator: &dyn Authenticator,
        request: &AuthRequest,
        auth_policy: &AuthPolicy,
        now: Instant,
    ) -> AuthOutcome {
        let entry = self.locks.entry(request.session_id).or_default();
        if entry.state == LockState::LockedOut {
            return AuthOutcome::LockedOut {
                retry_after_secs: auth_policy.lockout.as_secs(),
            };
        }

        let outcome = check(authenticator, request, auth_policy, &mut entry.attempts);
        match &outcome {
            AuthOutcome::Success => entry.state = LockState::Unlocked,
            AuthOutcome::LockedOut { .. } => {
                entry.state = LockState::LockedOut;
                self.timeouts
                    .start_lockout(request.session_id, now, auth_policy.lockout);
            }
            AuthOutcome::Failure { .. } | AuthOutcome::Error(_) => {}
        }
        outcome
    }

    /// Called when a lockout timer (`timeouts.expired_lockouts`)
    /// elapses: the session goes back to plain `Locked` so the user
    /// can try again, rather than back to fully `Unlocked`.
    pub fn clear_lockout(&mut self, session: SessionId) {
        if let Some(lock) = self.locks.get_mut(&session) {
            if lock.state == LockState::LockedOut {
                lock.state = LockState::Locked;
                lock.attempts = 0;
            }
        }
    }

    pub fn add_inhibitor(
        &mut self,
        what: InhibitWhat,
        who: impl Into<String>,
        why: impl Into<String>,
        mode: InhibitMode,
    ) -> u64 {
        self.inhibitors.add(what, who, why, mode)
    }

    pub fn release_inhibitor(&mut self, id: u64) -> Result<()> {
        self.inhibitors.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct AlwaysFail;
    impl Authenticator for AlwaysFail {
        fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
            Err(SessionError::AuthFailed("nope".into()))
        }
    }

    fn auth_policy() -> AuthPolicy {
        AuthPolicy {
            pam_service: "test".into(),
            allow_empty_password: false,
            max_attempts: 2,
            lockout: Duration::from_secs(10),
        }
    }

    fn lock_policy() -> LockPolicy {
        LockPolicy {
            enabled: true,
            lock_on_idle: true,
            lock_on_suspend: true,
            grace_period: Duration::from_secs(5),
        }
    }

    #[test]
    fn repeated_failures_lock_out_further_attempts() {
        let mut mgr = LockManager::new();
        mgr.lock(1, &lock_policy()).unwrap();
        let req = AuthRequest {
            session_id: 1,
            user_name: "alice".into(),
            password: "wrong".into(),
        };
        let now = Instant::now();

        let auth_policy = auth_policy();
        mgr.attempt_unlock(&AlwaysFail, &req, &auth_policy, now);
        let outcome = mgr.attempt_unlock(&AlwaysFail, &req, &auth_policy, now);
        assert!(matches!(outcome, AuthOutcome::LockedOut { .. }));

        // A third attempt while locked out is rejected without even
        // touching the authenticator.
        let outcome = mgr.attempt_unlock(&AlwaysFail, &req, &auth_policy, now);
        assert!(matches!(outcome, AuthOutcome::LockedOut { .. }));
    }

    #[test]
    fn inhibitor_blocks_locking() {
        let mut mgr = LockManager::new();
        mgr.add_inhibitor(
            InhibitWhat::Lock,
            "kiosk-app",
            "kiosk mode",
            InhibitMode::Block,
        );
        assert!(mgr.lock(1, &lock_policy()).is_err());
    }
}
