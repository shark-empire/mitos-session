use super::application::Application;
use crate::errors::Result;
use crate::session::Environment;
use crate::user::User;
use std::process::Child;

/// Fallback terminal, launched if a session's compositor keeps
/// crashing past its restart backoff -- gives the user something to
/// debug from instead of being dropped straight back to a login
/// prompt.
pub fn spawn_fallback_terminal(user: &User, env: &Environment) -> Result<Child> {
    Application::new(user.shell.to_string_lossy()).spawn(user, env)
}
