use mitos_session::authentication::{AuthOutcome, AuthPolicy, AuthRequest, Authenticator};
use mitos_session::errors::{Result, SessionError};
use mitos_session::lock::{InhibitMode, InhibitWhat, LockManager, LockPolicy};
use std::time::{Duration, Instant};

struct AlwaysFail;
impl Authenticator for AlwaysFail {
    fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
        Err(SessionError::AuthFailed("nope".into()))
    }
}

struct AlwaysSucceed;
impl Authenticator for AlwaysSucceed {
    fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
        Ok(())
    }
}

fn lock_policy() -> LockPolicy {
    LockPolicy {
        enabled: true,
        lock_on_idle: true,
        lock_on_suspend: true,
        grace_period: Duration::from_secs(0),
    }
}

fn auth_policy() -> AuthPolicy {
    AuthPolicy {
        pam_service: "test".into(),
        allow_empty_password: false,
        max_attempts: 2,
        lockout: Duration::from_secs(5),
    }
}

#[test]
fn full_lock_unlock_cycle() {
    let mut mgr = LockManager::new();
    mgr.lock(1, &lock_policy()).unwrap();
    assert!(mgr.is_locked(1));

    let req = AuthRequest { session_id: 1, user_name: "alice".into(), password: "hunter2".into() };
    let outcome = mgr.attempt_unlock(&AlwaysSucceed, &req, &auth_policy(), Instant::now());
    assert_eq!(outcome, AuthOutcome::Success);
    assert!(!mgr.is_locked(1));
}

#[test]
fn inhibitor_blocks_lock_and_release_restores_it() {
    let mut mgr = LockManager::new();
    let id = mgr.add_inhibitor(InhibitWhat::Lock, "kiosk", "kiosk mode", InhibitMode::Block);
    assert!(mgr.lock(1, &lock_policy()).is_err());

    mgr.release_inhibitor(id).unwrap();
    assert!(mgr.lock(1, &lock_policy()).is_ok());
}

#[test]
fn lockout_after_max_failed_attempts_blocks_further_tries() {
    let mut mgr = LockManager::new();
    mgr.lock(1, &lock_policy()).unwrap();
    let req = AuthRequest { session_id: 1, user_name: "alice".into(), password: "wrong".into() };
    let policy = auth_policy();

    mgr.attempt_unlock(&AlwaysFail, &req, &policy, Instant::now());
    let outcome = mgr.attempt_unlock(&AlwaysFail, &req, &policy, Instant::now());
    assert!(matches!(outcome, AuthOutcome::LockedOut { .. }));

    // A third attempt is rejected without even reaching the authenticator.
    let outcome = mgr.attempt_unlock(&AlwaysFail, &req, &policy, Instant::now());
    assert!(matches!(outcome, AuthOutcome::LockedOut { .. }));
}
