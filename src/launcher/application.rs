use crate::errors::Result;
use crate::session::Environment;
use crate::user::{supplementary_gids, User};
use std::process::{Child, Command, Stdio};

/// A single program to launch on behalf of a session. The compositor,
/// an autostart entry, and the fallback terminal are all just an
/// `Application` with a different command line.
#[derive(Debug, Clone)]
pub struct Application {
    pub command: String,
    pub args: Vec<String>,
    /// Restart automatically if it exits unexpectedly. Set for the
    /// compositor (losing it shouldn't end the session outright --
    /// see docs/session-lifecycle.md) and left off for one-shot
    /// autostart entries.
    pub restart_on_exit: bool,
}

impl Application {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            restart_on_exit: false,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn restart_on_exit(mut self, restart: bool) -> Self {
        self.restart_on_exit = restart;
        self
    }

    /// Spawn this application as `user`, with `env` applied on top of
    /// a clean environment (`env_clear`) rather than inheriting
    /// mitos-session's own -- a session's processes should never see
    /// the daemon's environment.
    pub fn spawn(&self, user: &User, env: &Environment) -> Result<Child> {
        let mut cmd = Command::new(&self.command);
        cmd.args(&self.args)
            .env_clear()
            .current_dir(&user.home)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        for (key, value) in env.iter() {
            cmd.env(key, value);
        }

        // Resolved here, in the parent, so the forked child's
        // `pre_exec` hook below only ever makes raw
        // setgroups/setgid/setuid syscalls -- no file I/O between
        // fork() and exec(), which is the property that actually
        // matters for fork safety in a multi-threaded daemon.
        let gids = supplementary_gids(&user.name)?;
        let uid = user.uid;
        let gid = user.gid;

        // SAFETY: this closure runs in the forked child before exec,
        // with no other threads sharing its address space. It only
        // performs the three syscalls below, using data resolved in
        // the parent above -- no allocation or I/O happens here, so
        // the usual fork()-in-a-threaded-process hazards don't apply.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(move || {
                nix::unistd::setgroups(&gids).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                nix::unistd::setgid(gid).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                nix::unistd::setuid(uid).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            });
        }

        Ok(cmd.spawn()?)
    }
}
