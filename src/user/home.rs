use super::user::User;
use crate::errors::{Result, SessionError};
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

/// Sanity-check (and if necessary, create) a user's home directory
/// before starting a session in it. Refuses to hand a session a home
/// directory it does not actually own -- a stale or misconfigured home
/// owned by the wrong uid is a privilege-escalation footgun, not
/// something to silently chown and continue past.
pub fn ensure_home_ready(user: &User) -> Result<()> {
    match fs::metadata(&user.home) {
        Ok(meta) => {
            if meta.uid() != user.uid.as_raw() {
                return Err(SessionError::PermissionDenied(format!(
                    "home directory {} is not owned by {}",
                    user.home.display(),
                    user.name
                )));
            }
            if !meta.is_dir() {
                return Err(SessionError::Config(format!(
                    "home directory {} is not a directory",
                    user.home.display()
                )));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&user.home)?;
            fs::set_permissions(&user.home, fs::Permissions::from_mode(0o700))?;
            std::os::unix::fs::chown(&user.home, Some(user.uid.as_raw()), Some(user.gid.as_raw()))?;
            tracing::info!(user = %user.name, home = %user.home.display(), "created missing home directory");
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
