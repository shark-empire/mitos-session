use crate::config::{AuthSettings, LockSettings};
use std::time::Duration;

/// Runtime authentication policy, built once from config: which PAM
/// service to authenticate against, and how many failures are
/// tolerated before a lockout (shared with `lock::policy`, since "too
/// many bad passwords" and "how long the screen stays locked" are the
/// same knob from the user's point of view).
#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub pam_service: String,
    pub allow_empty_password: bool,
    pub max_attempts: u32,
    pub lockout: Duration,
}

impl AuthPolicy {
    pub fn new(auth: &AuthSettings, lock: &LockSettings) -> Self {
        Self {
            pam_service: auth.pam_service.clone(),
            allow_empty_password: auth.allow_empty_password,
            max_attempts: lock.max_auth_attempts,
            lockout: Duration::from_secs(lock.lockout_secs),
        }
    }
}
