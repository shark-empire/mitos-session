use super::session::{SessionId, SessionType};
use crate::errors::Result;
use crate::user::User;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The environment variables a session's processes (compositor,
/// autostart apps, fallback terminal) are launched with. A `BTreeMap`
/// rather than a `HashMap` purely so `docs`/debug output and tests get
/// a stable, readable order.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    vars: BTreeMap<String, String>,
}

impl Environment {
    /// Build the standard XDG-flavoured environment for a new session.
    /// Returns the environment plus the `XDG_RUNTIME_DIR` path it
    /// computed, so the caller can create that directory.
    pub fn for_session(
        user: &User,
        session_id: SessionId,
        session_type: SessionType,
        seat_id: &str,
        runtime_base: &Path,
    ) -> (Self, PathBuf) {
        let runtime_dir = runtime_base.join(user.uid.to_string());

        let mut env = Self::default();
        env.set("HOME", user.home.to_string_lossy());
        env.set("USER", &user.name);
        env.set("LOGNAME", &user.name);
        env.set("SHELL", user.shell.to_string_lossy());
        env.set("XDG_RUNTIME_DIR", runtime_dir.to_string_lossy());
        env.set("XDG_SESSION_ID", session_id.to_string());
        env.set("XDG_SEAT", seat_id);
        env.set("XDG_SESSION_TYPE", session_type.as_xdg_str());
        env.set("PATH", "/usr/local/bin:/usr/bin:/bin");

        (env, runtime_dir)
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Create `XDG_RUNTIME_DIR` with the `0700` permissions the XDG
    /// Base Directory spec requires, owned by the session's user.
    pub fn ensure_runtime_dir(path: &Path, user: &User) -> Result<()> {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        std::os::unix::fs::chown(path, Some(user.uid.as_raw()), Some(user.gid.as_raw()))?;
        Ok(())
    }
}
