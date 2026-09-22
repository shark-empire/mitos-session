use crate::authentication::policy::AuthPolicy;
use crate::authentication::request::AuthRequest;
use crate::errors::{Result, SessionError};
use zeroize::ZeroizingString;

/// The outcome of an authentication attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Success,
    Failed { attempts_remaining: u32 },
    LockedOut { retry_after_secs: u64 },
    Error(String),
}

/// Trait for underlying authentication backends (e.g., PAM, mock).
pub trait Authenticator {
    fn authenticate(&self, req: &AuthRequest) -> Result<AuthOutcome>;
}

/// Real PAM authenticator.
pub struct PamAuthenticator {
    pub service: String,
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
        // We use a block to ensure PAM objects are dropped and zeroized immediately on error.
        let result = (|| -> Result<AuthOutcome> {
            let mut auth = pam::Authenticator::with_password(&self.service)
                .map_err(|e| SessionError::Protocol(format!("PAM init failed: {e}")))?;

            let pam_password = pam::Password::from(req.password.as_str());

            auth.get_handler()
                .set_user(&req.user_name, Some(pam_password))
                .map_err(|e| SessionError::Protocol(format!("PAM set_user failed: {e}")))?;

            match auth.authenticate() {
                Ok(_) => {
                    if let Err(e) = auth.open_session() {
                        tracing::warn!(error = %e, "PAM open_session failed after successful auth");
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
                    Ok(AuthOutcome::Failed { attempts_remaining: 0 }) 
                }
            }
        })();

        match result {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                tracing::error!(error = %e, "PAM authenticator error");
                Ok(AuthOutcome::Error(e.to_string()))
            }
        }
    }
}

/// Orchestrates an authentication attempt against a policy.
pub fn check(
    authenticator: &dyn Authenticator,
    req: &AuthRequest,
    policy: &AuthPolicy,
    attempts: &mut u32,
) -> AuthOutcome {
    // 1. Check empty password policy
    if !policy.allow_empty_password && req.password.is_empty() {
        *attempts += 1;
    } else {
        // 2. Delegate to backend
        match authenticator.authenticate(req) {
            Ok(AuthOutcome::Success) => {
                *attempts = 0;
                return AuthOutcome::Success;
            }
            Ok(AuthOutcome::Failed { .. }) | Err(_) => {
                *attempts += 1;
            }
            Ok(other) => return other, // Pass through Error from backend
        }
    }

    // 3. Check if locked out after this attempt
    if *attempts >= policy.max_attempts {
        // Format session_id to string for the deterministic jitter hash
        let session_id_str = req.session_id.to_string(); 
        let duration = policy.lockout_duration(*attempts, &session_id_str);
        
        return AuthOutcome::LockedOut {
            retry_after_secs: duration.as_secs(),
        };
    }

    let remaining = policy.max_attempts.saturating_sub(*attempts);
    AuthOutcome::Failed { attempts_remaining: remaining }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct AlwaysFail;
    impl Authenticator for AlwaysFail {
        fn authenticate(&self, _request: &AuthRequest) -> Result<AuthOutcome> {
            Ok(AuthOutcome::Failed { attempts_remaining: 0 })
        }
    }

    struct AlwaysSucceed;
    impl Authenticator for AlwaysSucceed {
        fn authenticate(&self, _request: &AuthRequest) -> Result<AuthOutcome> {
            Ok(AuthOutcome::Success)
        }
    }

    fn request() -> AuthRequest {
        AuthRequest {
            session_id: 1, // Assuming SessionId implements Display
            user_name: "alice".into(),
            password: ZeroizingString::from("hunter2"),
        }
    }

    fn test_policy() -> AuthPolicy {
        AuthPolicy {
            pam_service: "test".into(),
            allow_empty_password: false,
            max_attempts: 3,
            base_lockout: Duration::from_secs(30),
            max_lockout: Duration::from_secs(300),
        }
    }

    #[test]
    fn locks_out_after_max_attempts() {
        let policy = test_policy();
        let mut attempts = 0;
        
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::Failed { attempts_remaining: 2 }
        ));
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::Failed { attempts_remaining: 1 }
        ));
        
        // 3rd attempt triggers lockout. 
        // Base is 30s, plus up to 20% jitter (6s). 
        // So retry_after_secs should be between 30 and 36.
        match check(&AlwaysFail, &request(), &policy, &mut attempts) {
            AuthOutcome::LockedOut { retry_after_secs } => {
                assert!(retry_after_secs >= 30 && retry_after_secs <= 36);
            }
            _ => panic!("Expected LockedOut"),
        }
    }

    #[test]
    fn success_resets_attempt_counter() {
        let policy = test_policy();
        let mut attempts = 2;
        assert_eq!(
            check(&AlwaysSucceed, &request(), &policy, &mut attempts),
            AuthOutcome::Success
        );
        assert_eq!(attempts, 0);
    }
}
