use super::policy::AuthPolicy;
use super::request::AuthRequest;
use super::result::AuthOutcome;
use crate::errors::{Result, SessionError};

/// Anything that can check a credential. A trait mainly so tests (and
/// a future non-PAM backend, e.g. for CI containers with no PAM stack
/// configured) can substitute a fake implementation instead of
/// talking to the real one.
pub trait Authenticator {
    fn authenticate(&self, request: &AuthRequest) -> Result<()>;
}

/// The real backend: authenticates against Linux-PAM under the
/// configured service name (`/etc/pam.d/<pam_service>`), the same
/// mechanism `login`, `sudo`, and every other real authenticator on
/// the box uses -- mitos-session never sees a password hash itself.
///
/// NOTE: the exact `pam` crate API surface used below (`with_password`,
/// `conversation_mut().set_credentials`, `authenticate`,
/// `open_session`) matches the common pattern for that crate, but this
/// box has no network access to check it against whatever version
/// `cargo update` actually resolves -- verify against the installed
/// version before relying on this in anger.
pub struct PamAuthenticator {
    service: String,
}

impl PamAuthenticator {
    pub fn new(policy: &AuthPolicy) -> Self {
        Self {
            service: policy.pam_service.clone(),
        }
    }
}

impl Authenticator for PamAuthenticator {
    fn authenticate(&self, request: &AuthRequest) -> Result<()> {
        if request.password.is_empty() {
            return Err(SessionError::AuthFailed("empty password".into()));
        }

        let mut client = pam::Client::with_password(&self.service)
            .map_err(|e| SessionError::Pam(e.to_string()))?;
        client
            .conversation_mut()
            .set_credentials(&request.user_name, &request.password);
        client
            .authenticate()
            .map_err(|e| SessionError::AuthFailed(e.to_string()))?;
        client
            .open_session()
            .map_err(|e| SessionError::Pam(e.to_string()))?;
        Ok(())
    }
}

/// Runs `authenticator` against `request`, translating the result into
/// the attempt-tracking `AuthOutcome` the IPC layer sends back, and
/// mutating the caller-supplied attempt counter (owned by
/// `lock::LockManager` per-session, not by this module).
pub fn check(
    authenticator: &dyn Authenticator,
    request: &AuthRequest,
    policy: &AuthPolicy,
    attempts_so_far: &mut u32,
) -> AuthOutcome {
    match authenticator.authenticate(request) {
        Ok(()) => {
            *attempts_so_far = 0;
            AuthOutcome::Success
        }
        Err(e) => {
            *attempts_so_far += 1;
            tracing::warn!(user = %request.user_name, error = %e, "authentication attempt failed");
            if *attempts_so_far >= policy.max_attempts {
                AuthOutcome::LockedOut {
                    retry_after_secs: policy.lockout.as_secs(),
                }
            } else {
                AuthOutcome::Failure {
                    attempts_remaining: policy.max_attempts - *attempts_so_far,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct AlwaysFail;
    impl Authenticator for AlwaysFail {
        fn authenticate(&self, _request: &AuthRequest) -> Result<()> {
            Err(SessionError::AuthFailed("nope".into()))
        }
    }

    struct AlwaysSucceed;
    impl Authenticator for AlwaysSucceed {
        fn authenticate(&self, _request: &AuthRequest) -> Result<()> {
            Ok(())
        }
    }

    fn request() -> AuthRequest {
        AuthRequest {
            session_id: 1,
            user_name: "alice".into(),
            password: "hunter2".into(),
        }
    }

    #[test]
    fn locks_out_after_max_attempts() {
        let policy = AuthPolicy {
            pam_service: "test".into(),
            allow_empty_password: false,
            max_attempts: 3,
            lockout: Duration::from_secs(30),
        };
        let mut attempts = 0;
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::Failure {
                attempts_remaining: 2
            }
        ));
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::Failure {
                attempts_remaining: 1
            }
        ));
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::LockedOut {
                retry_after_secs: 30
            }
        ));
    }

    #[test]
    fn success_resets_attempt_counter() {
        let policy = AuthPolicy {
            pam_service: "test".into(),
            allow_empty_password: false,
            max_attempts: 3,
            lockout: Duration::from_secs(30),
        };
        let mut attempts = 2;
        assert_eq!(
            check(&AlwaysSucceed, &request(), &policy, &mut attempts),
            AuthOutcome::Success
        );
        assert_eq!(attempts, 0);
    }
}
