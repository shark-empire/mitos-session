use crate::authentication::request::AuthRequest;
use crate::errors::{Result, SessionError};
use std::time::Duration;
use zeroize::ZeroizingString;

/// The outcome of an authentication attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthOutcome {
    Success,
    Failed { attempts_remaining: u32 },
    LockedOut { retry_after_secs: u64 },
    Error(String),
}

/// The policy governing authentication attempts and lockouts.
#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub pam_service: String,
    pub allow_empty_password: bool,
    pub max_attempts: u32,
    pub lockout: Duration,
}

/// Trait for underlying authentication backends (e.g., PAM, mock).
pub trait Authenticator {
    fn authenticate(&self, req: &AuthRequest) -> Result<AuthOutcome>;
}

/// Real PAM authenticator.
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
        // Note: The exact API depends on the specific `pam` crate version/fork you are using.
        // This assumes a crate that provides `Authenticator::with_password` and `Password::from`.
        
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
                    // Return a generic Failed; the `check` function will calculate remaining attempts
                    Ok(AuthOutcome::Failed { attempts_remaining: 0 }) 
                }
            }
        })();

        // `auth` and `pam_password` are dropped here, zeroizing C memory.
        // `req.password` is dropped by the caller when AuthRequest goes out of scope.
        
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
/// 
/// This function handles the attempt counting, empty password checks, 
/// and lockout logic, delegating the actual credential verification 
/// to the provided `Authenticator`.
pub fn check(
    authenticator: &dyn Authenticator,
    req: &AuthRequest,
    policy: &AuthPolicy,
    attempts: &mut u32,
) -> AuthOutcome {
    // 1. Check if already locked out
    if *attempts >= policy.max_attempts {
        return AuthOutcome::LockedOut {
            retry_after_secs: policy.lockout.as_secs(),
        };
    }

    // 2. Check empty password policy
    if !policy.allow_empty_password && req.password.is_empty() {
        *attempts += 1;
        let remaining = policy.max_attempts.saturating_sub(*attempts);
        return AuthOutcome::Failed { attempts_remaining: remaining };
    }

    // 3. Delegate to backend
    match authenticator.authenticate(req) {
        Ok(AuthOutcome::Success) => {
            *attempts = 0;
            AuthOutcome::Success
        }
        Ok(AuthOutcome::Failed { .. }) | Err(_) => {
            *attempts += 1;
            if *attempts >= policy.max_attempts {
                AuthOutcome::LockedOut {
                    retry_after_secs: policy.lockout.as_secs(),
                }
            } else {
                let remaining = policy.max_attempts.saturating_sub(*attempts);
                AuthOutcome::Failed { attempts_remaining: remaining }
            }
        }
        Ok(other) => other, // Pass through LockedOut or Error from backend
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            session_id: 1, // Adjust if your SessionId is a String/Uuid
            user_name: "alice".into(),
            password: ZeroizingString::from("hunter2"),
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
            AuthOutcome::Failed { attempts_remaining: 2 }
        ));
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::Failed { attempts_remaining: 1 }
        ));
        assert!(matches!(
            check(&AlwaysFail, &request(), &policy, &mut attempts),
            AuthOutcome::LockedOut { retry_after_secs: 30 }
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
