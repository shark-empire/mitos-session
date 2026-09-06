use super::user::User;
use std::time::SystemTime;

/// Higher-level view of an account than the raw passwd entry: adds the
/// bits of state mitos-session itself tracks (last login, whether the
/// account is currently locked out of authentication) on top of the
/// static `User` record.
#[derive(Debug, Clone)]
pub struct Account {
    pub user: User,
    pub last_login: Option<SystemTime>,
    pub failed_attempts: u32,
    pub locked_until: Option<SystemTime>,
}

impl Account {
    pub fn new(user: User) -> Self {
        Self {
            user,
            last_login: None,
            failed_attempts: 0,
            locked_until: None,
        }
    }

    pub fn is_locked_out(&self, now: SystemTime) -> bool {
        matches!(self.locked_until, Some(until) if until > now)
    }

    pub fn record_success(&mut self, now: SystemTime) {
        self.failed_attempts = 0;
        self.locked_until = None;
        self.last_login = Some(now);
    }

    pub fn record_failure(&mut self) {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
    }
}
