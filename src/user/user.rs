use crate::errors::{Result, SessionError};
use nix::unistd::{Gid, Uid};
use std::path::PathBuf;

/// A resolved system account, as read from `/etc/passwd`.
#[derive(Debug, Clone)]
pub struct User {
    pub uid: Uid,
    pub gid: Gid,
    pub name: String,
    pub home: PathBuf,
    pub shell: PathBuf,
}

impl User {
    /// Look up an account by login name.
    pub fn by_name(name: &str) -> Result<Self> {
        let entry = nix::unistd::User::from_name(name)
            .map_err(SessionError::from)?
            .ok_or_else(|| SessionError::UnknownUser(name.to_string()))?;
        Ok(Self::from_nix(entry))
    }

    /// Look up an account by numeric uid.
    pub fn by_uid(uid: Uid) -> Result<Self> {
        let entry = nix::unistd::User::from_uid(uid)
            .map_err(SessionError::from)?
            .ok_or_else(|| SessionError::UnknownUser(uid.to_string()))?;
        Ok(Self::from_nix(entry))
    }

    fn from_nix(entry: nix::unistd::User) -> Self {
        Self {
            uid: entry.uid,
            gid: entry.gid,
            name: entry.name,
            home: entry.dir,
            shell: entry.shell,
        }
    }

    /// True for uid 0. A handful of privileged IPC requests (managing
    /// another user's session, forcing a reboot while someone else is
    /// logged in) are gated on this rather than a bespoke ACL.
    pub fn is_root(&self) -> bool {
        self.uid.is_root()
    }
}
