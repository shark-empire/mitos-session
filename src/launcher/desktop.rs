use super::application::Application;
use crate::errors::Result;
use crate::session::Environment;
use crate::user::User;
use std::process::Child;

/// Launch every configured autostart entry for a session once its
/// compositor is confirmed ready. MITOS doesn't parse XDG `.desktop`
/// files yet (see README roadmap) -- entries are plain command lines
/// from config in the meantime.
pub fn spawn_autostart(user: &User, env: &Environment, commands: &[String]) -> Result<Vec<Child>> {
    commands
        .iter()
        .map(|line| {
            let mut parts = line.split_whitespace();
            let program = parts.next().unwrap_or_default();
            let mut app = Application::new(program);
            for arg in parts {
                app = app.arg(arg);
            }
            app.spawn(user, env)
        })
        .collect()
}
