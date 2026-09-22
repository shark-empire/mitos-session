use std::collections::HashMap;
use std::process::Child;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::config::SessionSettings;
use crate::errors::{Result, SessionError};
use crate::ipc::ConnId;
use crate::session::environment::Environment;
use crate::user::User;

/// Unique identifier for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The type of display server/session environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionType {
    Wayland,
    X11,
    Tty,
}

impl SessionType {
    pub fn as_xdg_str(&self) -> &'static str {
        match self {
            SessionType::Wayland => "wayland",
            SessionType::X11 => "x11",
            SessionType::Tty => "tty",
        }
    }
}

/// The lifecycle state of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Active,
    Locked,
    Suspended,
    Closing,
    Closed,
}

impl SessionState {
    pub fn is_locked(&self) -> bool {
        matches!(self, SessionState::Locked | SessionState::Suspended)
    }
}

/// The core session data.
pub struct Session {
    pub id: SessionId,
    pub uid: nix::unistd::Uid,
    pub user_name: String,
    pub seat_id: String,
    pub session_type: SessionType,
    pub created_at: SystemTime,
}

/// Phase 4: Recovery cascade state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartCascade {
    Compositor,
    Shell,
    UserServices,
    Failed,
}

/// The runtime context for a session, including its processes and state.
pub struct SessionContext {
    pub session: Session,
    pub state: SessionState,
    pub environment: Environment,
    pub compositor_conn: Option<ConnId>,
    pub compositor_process: Option<Child>,
    
    // --- PHASE 4 ADDITIONS ---
    pub compositor_restarts: u32,
    pub shell_restarts: u32,
    pub cascade: RestartCascade,
    
    /// The process group ID (PGID) of the session's root process.
    /// Because we call `setsid()` when spawning, PID == PGID.
    /// This allows us to send SIGTERM/SIGKILL to the entire session tree.
    pub process_group: Option<libc::pid_t>,
}

impl SessionContext {
    pub fn new(session: Session, environment: Environment) -> Self {
        Self {
            session,
            state: SessionState::Starting,
            environment,
            compositor_conn: None,
            compositor_process: None,
            compositor_restarts: 0,
            shell_restarts: 0,
            cascade: RestartCascade::Compositor,
            process_group: None,
        }
    }

    pub fn id(&self) -> SessionId {
        self.session.id
    }

    pub fn transition(&mut self, new_state: SessionState) -> Result<()> {
        tracing::debug!(
            session_id = %self.id(), 
            from = ?self.state, 
            to = ?new_state, 
            "session state transition"
        );
        self.state = new_state;
        Ok(())
    }
}

/// Manages all active sessions.
pub struct SessionManager {
    sessions: HashMap<SessionId, SessionContext>,
    next_id: u64,
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
        _settings: &SessionSettings,
    ) -> Result<SessionId> {
        let id = SessionId(self.next_id);
        self.next_id += 1;

        // Build environment (delegating to environment.rs logic)
        let (env, runtime_dir) = Environment::for_session(
            &user,
            id,
            session_type,
            seat_id,
            std::path::Path::new("/run/user"),
        );

        // Ensure runtime directory exists
        Environment::ensure_runtime_dir(&runtime_dir, &user)
            .map_err(|e| SessionError::Protocol(format!("failed to create runtime dir: {e}")))?;

        let session = Session {
            id,
            uid: user.uid,
            user_name: user.name.clone(),
            seat_id: seat_id.to_string(),
            session_type,
            created_at: SystemTime::now(),
        };

        let ctx = SessionContext::new(session, env);
        self.sessions.insert(id, ctx);

        Ok(id)
    }

    pub fn get(&self, id: SessionId) -> Result<&SessionContext> {
        self.sessions.get(&id).ok_or(SessionError::UnknownSession(id))
    }

    pub fn get_mut(&mut self, id: SessionId) -> Result<&mut SessionContext> {
        self.sessions.get_mut(&id).ok_or(SessionError::UnknownSession(id))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut SessionContext> {
        self.sessions.values_mut()
    }

    pub fn list(&self) -> impl Iterator<Item = &SessionContext> {
        self.sessions.values()
    }

    pub fn terminate_session(&mut self, id: SessionId) -> Result<()> {
        if self.sessions.remove(&id).is_some() {
            Ok(())
        } else {
            Err(SessionError::UnknownSession(id))
        }
    }

    pub fn owner_uid(&self, id: SessionId) -> Result<u32> {
        self.get(id).map(|ctx| ctx.session.uid.as_raw())
    }
}
