use crate::errors::{Result, SessionError};

/// A session's position in its lifecycle. See
/// `docs/session-lifecycle.md` for the full state diagram and what
/// drives each transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// PAM session opened, environment built, compositor not yet
    /// confirmed ready -- either because it just started, or because
    /// it crashed and is being relaunched (see
    /// `launcher::decide_restart`).
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
                | (Starting, Locked)
                | (Active, Idle)
                | (Active, Locked)
                | (Active, Closing)
                | (Active, Starting)
                | (Idle, Active)
                | (Idle, Locked)
                | (Idle, Closing)
                | (Idle, Starting)
                | (Locked, Active)
                | (Locked, Closing)
                | (Locked, Starting)
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

    #[test]
    fn a_live_session_can_fall_back_to_starting_when_its_compositor_dies() {
        // This is the transition a compositor crash-and-restart takes
        // (see `launcher::decide_restart` / `Daemon::handle_compositor_exit`)
        // -- the session isn't gone, it just has no display for a moment.
        assert!(SessionState::Active.transition(SessionState::Starting).is_ok());
        assert!(SessionState::Idle.transition(SessionState::Starting).is_ok());
        assert!(SessionState::Locked.transition(SessionState::Starting).is_ok());
        // But you still can't skip straight back to Active --
        // RegisterCompositor deciding that depends on whether the
        // session is still actually locked (see the next test).
        assert!(SessionState::Starting.transition(SessionState::Idle).is_err());
    }

    #[test]
    fn a_relaunched_compositor_can_come_back_straight_into_locked() {
        // If the session was Locked when its compositor crashed
        // (Locked -> Starting), the fresh compositor instance that
        // registers afterward needs to be told to show the lock screen
        // again rather than the session silently coming back Active --
        // see `Daemon`'s `RegisterCompositor` handler in src/main.rs.
        assert!(SessionState::Starting.transition(SessionState::Locked).is_ok());
    }
}
