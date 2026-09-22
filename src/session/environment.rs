use super::session::{SessionId, SessionType};
use crate::errors::{Result, SessionError};
use crate::user::User;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The environment variables a session's processes (compositor,
/// autostart apps, fallback terminal) are launched with.
///
/// A `BTreeMap` rather than a `HashMap` purely so docs/debug output
/// and tests get a stable, readable order.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    vars: BTreeMap<String, String>,
}

impl Environment {
    /// Build the standard XDG-flavoured environment for a new session.
    ///
    /// Returns the environment plus the `XDG_RUNTIME_DIR` path it
    /// computed, so the caller can create that directory.
    ///
    /// This constructor deliberately does *not* pass through the
    /// daemon's full environment. Only a small allowlist of safe
    /// locale/input-method/terminal variables is inherited.
    pub fn for_session(
        user: &User,
        session_id: SessionId,
        session_type: SessionType,
        seat_id: &str,
        runtime_base: &Path,
    ) -> (Self, PathBuf) {
        let runtime_dir = runtime_base.join(user.uid.to_string());

        let mut env = Self::default();

        // Inherit only known-safe variables from the daemon environment.
        inherit_safe_environment(&mut env);

        // Mandatory safe PATH.
        env.set(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        );

        // Mandatory user identity variables.
        env.set("HOME", user.home.to_string_lossy());
        env.set("USER", &user.name);
        env.set("LOGNAME", &user.name);
        env.set("SHELL", user.shell.to_string_lossy());

        // Mandatory session variables.
        env.set("XDG_RUNTIME_DIR", runtime_dir.to_string_lossy());
        env.set("XDG_SESSION_ID", session_id.to_string());
        env.set("XDG_SEAT", seat_id);
        env.set("XDG_SESSION_TYPE", session_type.as_xdg_str());
        env.set("XDG_SESSION_CLASS", "user");
        env.set("XDG_SESSION_DESKTOP", "mitos");
        env.set("XDG_CURRENT_DESKTOP", "MITOS");

        // Display-server-specific variables.
        //
        // These are owned by the session, not inherited from whatever
        // environment mitos-session itself was started in.
        match session_type {
            SessionType::Wayland => {
                env.set("WAYLAND_DISPLAY", "wayland-0");
                env.vars.remove("DISPLAY");
            }
            SessionType::X11 => {
                env.set("DISPLAY", ":0");
                env.vars.remove("WAYLAND_DISPLAY");
            }
            SessionType::Tty => {
                env.vars.remove("DISPLAY");
                env.vars.remove("WAYLAND_DISPLAY");
            }
        }

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

    /// Create `XDG_RUNTIME_DIR` with the permissions and ownership
    /// required by the XDG Base Directory spec.
    ///
    /// This now delegates to the hardened runtime-directory logic in
    /// `crate::session::runtime`, which opens the final directory with
    /// `O_NOFOLLOW` and locks ownership/permissions through the fd.
    pub fn ensure_runtime_dir(path: &Path, user: &User) -> Result<()> {
        let base = path
            .parent()
            .ok_or_else(|| SessionError::Protocol("runtime directory has no parent".into()))?;

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| SessionError::Protocol("runtime directory has no file name".into()))?;

        let created =
            crate::session::runtime::ensure_named_runtime_dir(base, name, user.uid, user.gid)
                .map_err(|e| SessionError::Protocol(e.to_string()))?;

        if created != path {
            return Err(SessionError::Protocol(format!(
                "runtime directory path mismatch: expected {}, created {}",
                path.display(),
                created.display()
            )));
        }

        Ok(())
    }
}

fn inherit_safe_environment(env: &mut Environment) {
    for (key, value) in std::env::vars() {
        if is_safe_inherited_env(&key) {
            env.set(key, value);
        }
    }
}

/// Explicit allowlist of variables that may be inherited from the
/// daemon environment.
///
/// This deliberately rejects dangerous variables such as:
///
/// - `LD_PRELOAD`
/// - `LD_LIBRARY_PATH`
/// - `LD_AUDIT`
/// - `LD_DEBUG`
/// - `IFS`
/// - `ENV`
/// - `BASH_ENV`
/// - inherited `DISPLAY`
/// - inherited `WAYLAND_DISPLAY`
/// - inherited `XDG_RUNTIME_DIR`
fn is_safe_inherited_env(key: &str) -> bool {
    matches!(
        key,
        "LANG"
            | "LANGUAGE"
            | "LC_ALL"
            | "LC_CTYPE"
            | "LC_NUMERIC"
            | "LC_TIME"
            | "LC_COLLATE"
            | "LC_MONETARY"
            | "LC_MESSAGES"
            | "LC_PAPER"
            | "LC_NAME"
            | "LC_ADDRESS"
            | "LC_TELEPHONE"
            | "LC_MEASUREMENT"
            | "LC_IDENTIFICATION"
            | "TZ"
            | "TERM"
            | "GTK_IM_MODULE"
            | "QT_IM_MODULE"
            | "XMODIFIERS"
    )
}
