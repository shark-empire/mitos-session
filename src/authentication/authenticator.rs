use crate::authentication::request::AuthRequest;
use crate::errors::{Result, SessionError};
use pam::{Authenticator, Password};
use zeroize::ZeroizingString;

pub enum AuthOutcome {
    Success,
    Failed,
    LockedOut { retry_after_secs: u64 },
    Error(String),
}

pub trait Authenticator {
    fn authenticate(&self, req: &AuthRequest) -> Result<AuthOutcome>;
}

pub struct PamAuthenticator {
    service: String,
}

impl PamAuthenticator {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

impl Authenticator for PamAuthenticator {
    fn authenticate(&self, req: &AuthRequest) -> Result<AuthOutcome> {
        // 1. Initialize PAM Authenticator for the "mitos" service
        //    (expects /etc/pam.d/mitos-login or similar)
        let mut auth = match Authenticator::with_password(&self.service) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(error = %e, "failed to initialize PAM authenticator");
                return Err(SessionError::Protocol(format!("PAM init failed: {e}")));
            }
        };

        // 2. Set the user and password directly.
        //    Password::from() creates a secure PAM password object that 
        //    zeroizes its memory when dropped. We pass a reference to our
        //    ZeroizingString, which PAM will copy internally.
        let pam_password = Password::from(req.password.as_str());
        
        if let Err(e) = auth
            .get_handler()
            .set_user(&req.user_name, Some(pam_password))
        {
            tracing::error!(error = %e, user = %req.user_name, "PAM set_user failed");
            return Err(SessionError::Protocol(format!("PAM set_user failed: {e}")));
        }

        // 3. Authenticate
        match auth.authenticate() {
            Ok(_) => {
                // 4. Open session (establishes credentials, sets up env, etc.)
                if let Err(e) = auth.open_session() {
                    tracing::warn!(error = %e, "PAM open_session failed after successful auth");
                    // Depending on your PAM config, this might be fatal. 
                    // For now, we log it but consider auth successful.
                }
                Ok(AuthOutcome::Success)
            }
            Err(e) => {
                tracing::warn!(
                    target: "mitos_auth", 
                    user = %req.user_name, 
                    error = %e, 
                    "PAM authentication failed"
                );
                Ok(AuthOutcome::Failed)
            }
        }
        
        // When this function returns, `auth` and `pam_password` are dropped.
        // The `pam` crate automatically zeroizes the password buffer in C memory.
        // Our `req.password` (ZeroizingString) is dropped by the caller when AuthRequest goes out of scope.
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
