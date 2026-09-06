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
