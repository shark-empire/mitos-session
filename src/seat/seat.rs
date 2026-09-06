use super::device::Device;
use crate::session::SessionId;
use std::collections::VecDeque;

/// One seat: a set of input/output hardware and the queue of sessions
/// that want to run on it. Only `active_session` actually has its
/// devices; everyone else in `queued_sessions` is "logged in but
/// switched away from," the same as a locked VT.
#[derive(Debug, Clone)]
pub struct Seat {
    pub id: String,
    pub active_session: Option<SessionId>,
    pub queued_sessions: VecDeque<SessionId>,
    pub devices: Vec<Device>,
}

impl Seat {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            active_session: None,
            queued_sessions: VecDeque::new(),
            devices: Vec::new(),
        }
    }

    /// All sessions associated with this seat, active one first.
    pub fn sessions(&self) -> impl Iterator<Item = SessionId> + '_ {
        self.active_session
            .into_iter()
            .chain(self.queued_sessions.iter().copied())
    }

    pub fn has_session(&self, id: SessionId) -> bool {
        self.active_session == Some(id) || self.queued_sessions.contains(&id)
    }
}
