use super::application::Application;
use crate::errors::Result;
use crate::session::Environment;
use crate::user::User;
use std::process::Child;

/// Spawn mitos-gui for a session. The compositor is the one process
/// mitos-session restarts on unexpected exit (within a short backoff,
/// tracked by the caller) rather than tearing the whole session down
/// -- see docs/session-lifecycle.md.
pub fn spawn_compositor(user: &User, env: &Environment, binary: &str) -> Result<Child> {
    Application::new(binary).restart_on_exit(true).spawn(user, env)
}

/// What to do after a session's compositor process exits unexpectedly
/// (detected via `SIGCHLD`, not a graceful `TerminateSession`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Try launching the compositor again.
    Restart,
    /// Give up on the compositor for this session and fall back to a
    /// plain terminal instead, so the user has *something* to work
    /// from and debug rather than a session that silently went dark.
    FallbackToTerminal,
}

/// Decide what to do given how many times this session's compositor
/// has already been restarted. Pure and stateless on purpose -- the
/// actual respawning (and the restart counter it reads) lives on
/// `Daemon` in `src/main.rs`, this is just the policy call.
pub fn decide_restart(restarts_so_far: u32, max_restarts: u32) -> RestartDecision {
    if restarts_so_far < max_restarts {
        RestartDecision::Restart
    } else {
        RestartDecision::FallbackToTerminal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_until_the_limit_then_falls_back() {
        assert_eq!(decide_restart(0, 3), RestartDecision::Restart);
        assert_eq!(decide_restart(2, 3), RestartDecision::Restart);
        assert_eq!(decide_restart(3, 3), RestartDecision::FallbackToTerminal);
        assert_eq!(decide_restart(9, 3), RestartDecision::FallbackToTerminal);
    }

    #[test]
    fn a_zero_limit_never_restarts() {
        assert_eq!(decide_restart(0, 0), RestartDecision::FallbackToTerminal);
    }
}
