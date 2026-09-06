use mitos_session::authentication::{check, AuthPolicy, AuthRequest, Authenticator};
use mitos_session::config::{AuthSettings, LockSettings};
use mitos_session::errors::{Result, SessionError};
use std::time::Duration;

struct Toggle(bool);
impl Authenticator for Toggle {
    fn authenticate(&self, _r: &AuthRequest) -> Result<()> {
        if self.0 {
            Ok(())
        } else {
            Err(SessionError::AuthFailed("bad credential".into()))
        }
    }
}

#[test]
fn policy_pulls_attempt_limits_from_lock_settings() {
    let auth = AuthSettings { pam_service: "svc".into(), allow_empty_password: false };
    let lock = LockSettings { max_auth_attempts: 5, lockout_secs: 60, ..Default::default() };

    let policy = AuthPolicy::new(&auth, &lock);
    assert_eq!(policy.pam_service, "svc");
    assert_eq!(policy.max_attempts, 5);
    assert_eq!(policy.lockout, Duration::from_secs(60));
}

#[test]
fn check_counts_failures_and_resets_on_success() {
    let policy = AuthPolicy {
        pam_service: "x".into(),
        allow_empty_password: false,
        max_attempts: 3,
        lockout: Duration::from_secs(10),
    };
    let req = AuthRequest { session_id: 1, user_name: "bob".into(), password: "x".into() };
    let mut attempts = 0;

    check(&Toggle(false), &req, &policy, &mut attempts);
    assert_eq!(attempts, 1);
    check(&Toggle(false), &req, &policy, &mut attempts);
    assert_eq!(attempts, 2);
    check(&Toggle(true), &req, &policy, &mut attempts);
    assert_eq!(attempts, 0, "a successful attempt should reset the counter");
}
