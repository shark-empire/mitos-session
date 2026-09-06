use nix::unistd::Uid;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Unique handle for a session for as long as it's alive. Reused only
/// after a full daemon restart -- never recycled at runtime, so stale
/// IDs from a previous login can't accidentally address a new one.
pub type SessionId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionType {
    Wayland,
    X11,
    Tty,
}

impl SessionType {
    pub fn as_xdg_str(self) -> &'static str {
        match self {
            SessionType::Wayland => "wayland",
            SessionType::X11 => "x11",
            SessionType::Tty => "tty",
        }
    }
}

/// The lightweight, cloneable facts about a session -- everything
/// `ipc::messages::SessionInfo` needs to describe it to a client.
/// The full runtime record (user info, environment, child process
/// handle) lives in `SessionContext`.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: SessionId,
    pub uid: Uid,
    pub user_name: String,
    pub seat_id: String,
    pub session_type: SessionType,
    pub vt: Option<u32>,
    pub created_at: SystemTime,
}
