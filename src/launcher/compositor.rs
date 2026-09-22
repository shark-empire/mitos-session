use std::os::unix::process::CommandExt;
use std::process::Child;

use crate::errors::{Result, SessionError};
use crate::session::Environment;
use crate::user::User;

/// Spawn mitos-gui for a session. 
/// 
/// We use a `pre_exec` hook to do two critical things before the child executes:
/// 1. `setsid()`: Creates a new process group (PID == PGID). This allows 
///    `mitos-session` to cleanly terminate the entire session tree (compositor 
///    + all autostart apps) via negative PGID signaling during logout or crash recovery.
/// 2. Privilege Dropping: Drops from root to the session user's UID/GID.
pub fn spawn_compositor(user: &User, env: &Environment, binary: &str) -> Result<Child> {
    let mut cmd = std::process::Command::new(binary);
    
    // Apply session environment variables
    for (k, v) in env.iter() {
        cmd.env(k, v);
    }

    let uid = user.uid.as_raw();
    let gid = user.gid.as_raw();
    // Note: Adjust `.groups` or `.supplementary_groups` depending on your User struct definition
    let groups: Vec<libc::gid_t> = user.groups.iter().map(|g| g.as_raw()).collect();

    unsafe {
        cmd.pre_exec(move || {
            // 1. Phase 4: Isolate the process group
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            
            // 2. Phase 2/4: Drop privileges from root to the session user
            // Order matters: setgroups -> setgid -> setuid
            if libc::setgroups(groups.len(), groups.as_ptr()) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setresgid(gid, gid, gid) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setresuid(uid, uid, uid) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            
            Ok(())
        });
    }

    cmd.spawn().map_err(SessionError::Io)
}

/// Spawn a fallback terminal (e.g., xterm or alacritty) when the 
/// compositor crash-restart cascade is exhausted.
pub fn spawn_fallback_terminal(user: &User, env: &Environment) -> Result<Child> {
    // Change "xterm" to your preferred terminal (alacritty, foot, etc.)
    let mut cmd = std::process::Command::new("xterm"); 
    
    for (k, v) in env.iter() {
        cmd.env(k, v);
    }

    let uid = user.uid.as_raw();
    let gid = user.gid.as_raw();
    let groups: Vec<libc::gid_t> = user.groups.iter().map(|g| g.as_raw()).collect();

    unsafe {
        cmd.pre_exec(move || {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setgroups(groups.len(), groups.as_ptr()) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setresgid(gid, gid, gid) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::setresuid(uid, uid, uid) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

   let child = cmd.spawn().map_err(SessionError::Io)?;
    Ok(child)
}

/// What to do after a session's compositor process exits unexpectedly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartDecision {
    /// Try launching the compositor again.
    Restart,
    /// Give up on the compositor for this session and fall back to a
    /// plain terminal instead.
    FallbackToTerminal,
}

/// Decide what to do given how many times this session's compositor
/// has already been restarted. Pure and stateless on purpose.
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
