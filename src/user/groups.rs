use crate::errors::{Result, SessionError};
use nix::unistd::{Gid, User as NixUser};
use std::path::Path;

const GROUP_FILE: &str = "/etc/group";

/// Every group `name` belongs to, primary group first. Used to build
/// the supplementary group list before `setgroups(2)`/`setgid(2)` when
/// launching a session (see `user::permissions::drop_privileges`).
///
/// This reads `/etc/group` directly rather than going through NSS, so
/// it only sees local accounts -- fine for the accounts MITOS targets
/// today. A directory-backed (LDAP/SSSD) deployment would need a real
/// `getgrouplist(3)` FFI call instead; nix does not expose one.
pub fn supplementary_gids(name: &str) -> Result<Vec<Gid>> {
    let entry = NixUser::from_name(name)
        .map_err(SessionError::from)?
        .ok_or_else(|| SessionError::UnknownUser(name.to_string()))?;

    let mut gids = vec![entry.gid];
    for gid in gids_from_group_file(Path::new(GROUP_FILE), name)? {
        if !gids.contains(&gid) {
            gids.push(gid);
        }
    }
    Ok(gids)
}

/// Parse a `/etc/group`-formatted file (`name:passwd:gid:member,member`)
/// and return the gid of every group that lists `name` as a member.
/// Split out from `supplementary_gids` so it can be unit tested against
/// a fixture file instead of the real `/etc/group`.
fn gids_from_group_file(path: &Path, name: &str) -> Result<Vec<Gid>> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        // A missing group file is not fatal here -- just means no
        // supplementary groups beyond the primary one.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    let mut gids = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let (_group_name, _passwd, gid_field, members_field) = (
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
            fields.next().unwrap_or_default(),
        );
        let is_member = members_field.split(',').any(|m| m == name);
        if is_member {
            if let Ok(gid) = gid_field.parse::<u32>() {
                gids.push(Gid::from_raw(gid));
            }
        }
    }
    Ok(gids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_membership_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("group");
        std::fs::write(
            &path,
            "wheel:x:10:root,alice\nvideo:x:44:alice,bob\nempty:x:99:\n",
        )
        .unwrap();

        let gids = gids_from_group_file(&path, "alice").unwrap();
        assert_eq!(gids, vec![Gid::from_raw(10), Gid::from_raw(44)]);

        let gids = gids_from_group_file(&path, "carol").unwrap();
        assert!(gids.is_empty());
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let gids = gids_from_group_file(Path::new("/nonexistent/group"), "alice").unwrap();
        assert!(gids.is_empty());
    }
}
