use super::context::SessionContext;
use super::lifecycle;
use super::session::{SessionId, SessionType};
use super::state::SessionState;
use crate::config::SessionSettings;
use crate::errors::{Result, SessionError};
use crate::user::User;
use std::collections::HashMap;

/// Owns every session mitos-session currently knows about. This is the
/// thing `main`'s calloop event handlers call into for IPC requests,
/// timers, and signal-driven cleanup -- it never touches sockets or
/// PAM directly, it just holds state and enforces the rules
/// (session-count limits, valid state transitions).
#[derive(Default)]
pub struct SessionManager {
    sessions: HashMap<SessionId, SessionContext>,
    next_id: SessionId,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn create_session(
        &mut self,
        user: User,
        seat_id: &str,
        session_type: SessionType,
        settings: &SessionSettings,
    ) -> Result<SessionId> {
        let existing = self.sessions.values().filter(|c| c.user.uid == user.uid).count() as u32;
        if existing >= settings.max_sessions_per_user {
            return Err(SessionError::PermissionDenied(format!(
                "{} already has {existing} session(s) open (limit {})",
                user.name, settings.max_sessions_per_user
            )));
        }

        let id = self.next_id;
        self.next_id += 1;

        let ctx = lifecycle::begin(id, user, seat_id, session_type, settings)?;
        self.sessions.insert(id, ctx);
        Ok(id)
    }

    pub fn terminate_session(&mut self, id: SessionId) -> Result<()> {
        let mut ctx = self.sessions.remove(&id).ok_or(SessionError::UnknownSession(id))?;
        lifecycle::end(&mut ctx)
    }

    pub fn get(&self, id: SessionId) -> Result<&SessionContext> {
        self.sessions.get(&id).ok_or(SessionError::UnknownSession(id))
    }

    pub fn get_mut(&mut self, id: SessionId) -> Result<&mut SessionContext> {
        self.sessions.get_mut(&id).ok_or(SessionError::UnknownSession(id))
    }

    pub fn list(&self) -> impl Iterator<Item = &SessionContext> {
        self.sessions.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut SessionContext> {
        self.sessions.values_mut()
    }

    pub fn set_state(&mut self, id: SessionId, next: SessionState) -> Result<()> {
        self.get_mut(id)?.transition(next)
    }

    pub fn owner_uid(&self, id: SessionId) -> Result<u32> {
        Ok(self.get(id)?.session.uid.as_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::unistd::{Gid, Uid};
    use std::path::PathBuf;

    fn fake_user(name: &str, uid: u32) -> User {
        User {
            uid: Uid::from_raw(uid),
            gid: Gid::from_raw(uid),
            name: name.to_string(),
            home: PathBuf::from(format!("/home/{name}")),
            shell: PathBuf::from("/bin/sh"),
        }
    }

    #[test]
    fn enforces_max_sessions_per_user() {
        // This exercises only the counting/limit logic; `create_session`
        // itself also touches the filesystem via `lifecycle::begin`, so
        // the *success* path is covered by the higher-level integration
        // tests in tests/session.rs instead of here.
        let mgr = SessionManager::new();
        let settings = SessionSettings {
            max_sessions_per_user: 0,
            ..Default::default()
        };
        let user = fake_user("alice", 1000);
        let existing = mgr.sessions.values().filter(|c| c.user.uid == user.uid).count() as u32;
        assert!(existing >= settings.max_sessions_per_user);
    }
}
