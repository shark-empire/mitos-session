use super::environment::Environment;
use super::session::{Session, SessionId};
use super::state::SessionState;
use crate::errors::Result;
use crate::ipc::ConnId;
use crate::user::User;
use std::process::Child;

/// The manager's full internal record for a session -- everything
/// `SessionManager` needs to drive it, which is more than what's ever
/// serialized out over IPC (`ipc::messages::SessionInfo` is the public
/// subset built from this).
pub struct SessionContext {
    pub session: Session,
    pub user: User,
    pub state: SessionState,
    pub environment: Environment,
    /// Set once mitos-gui registers itself as this session's
    /// compositor over IPC.
    pub compositor_conn: Option<ConnId>,
    /// Handle to the spawned compositor process, kept so logout can
    /// reap it instead of leaving a zombie.
    pub compositor_process: Option<Child>,
}

impl SessionContext {
    pub fn id(&self) -> SessionId {
        self.session.id
    }

    pub fn transition(&mut self, next: SessionState) -> Result<()> {
        self.state = self.state.transition(next)?;
        Ok(())
    }
}

impl std::fmt::Debug for SessionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionContext")
            .field("session", &self.session)
            .field("state", &self.state)
            .field("compositor_conn", &self.compositor_conn)
            .field("compositor_running", &self.compositor_process.is_some())
            .finish()
    }
}
