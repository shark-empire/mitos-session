use crate::errors::{Result, SessionError};

/// A session's position in its lifecycle. See
/// `docs/session-lifecycle.md` for the full state diagram and what
/// drives each transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// PAM session opened, environment built, compositor not yet
    /// confirmed ready.
    Starting,
    /// Compositor is up and this session owns its seat's input/output.
    Active,
    /// Active, but idle-timeout has fired short of the lock threshold
    /// (e.g. the screen has dimmed).
    Idle,
    /// Lock screen is up; only a successful authentication can leave
    /// this state.
    Locked,
    /// Logout in progress -- compositor and child processes are being
    /// torn down.
    Closing,
    /// Fully torn down; about to be removed from `SessionManager`.
    Closed,
}

impl SessionState {
    pub fn can_transition_to(self, next: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Starting, Active)
                | (Starting, Closing)
                | (Active, Idle)
                | (Active, Locked)
                | (Active, Closing)
                | (Idle, Active)
                | (Idle, Locked)
                | (Idle, Closing)
                | (Locked, Active)
                | (Locked, Closing)
                | (Closing, Closed)
        )
    }

    pub fn transition(self, next: SessionState) -> Result<SessionState> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(SessionError::InvalidTransition(format!("{self:?} -> {next:?}")))
        }
    }

    pub fn is_locked(self) -> bool {
        matches!(self, SessionState::Locked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_transitions_succeed() {
        assert!(SessionState::Starting.transition(SessionState::Active).is_ok());
        assert!(SessionState::Active.transition(SessionState::Locked).is_ok());
        assert!(SessionState::Locked.transition(SessionState::Active).is_ok());
        assert!(SessionState::Closing.transition(SessionState::Closed).is_ok());
    }

    #[test]
    fn skipping_closing_is_rejected() {
        assert!(SessionState::Active.transition(SessionState::Closed).is_err());
    }

    #[test]
    fn closed_is_terminal() {
        assert!(SessionState::Closed.transition(SessionState::Active).is_err());
    }
}
