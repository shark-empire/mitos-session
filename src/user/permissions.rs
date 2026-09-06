use super::groups::supplementary_gids;
use super::user::User;
use crate::errors::Result;
use nix::unistd::{setgid, setgroups, setuid};

/// Drop from the daemon's root privileges down to `user` before
/// `exec`ing anything on their behalf (the compositor, autostart
/// applications, a fallback terminal). Must be called from the child
/// process *after* fork and *before* exec -- never in the daemon's own
/// main thread.
///
/// Order matters: supplementary groups and the primary gid must be set
/// while we still have `CAP_SETGID`, which we lose the moment
/// `setuid` succeeds.
pub fn drop_privileges(user: &User) -> Result<()> {
    let gids = supplementary_gids(&user.name)?;
    setgroups(&gids)?;
    setgid(user.gid)?;
    setuid(user.uid)?;
    Ok(())
}
