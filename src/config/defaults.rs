//! `Default` impls for every settings struct. Kept separate from
//! `settings.rs` so the schema and its fallback values can be read
//! (and changed) independently -- these are the values `session.toml`
//! ships with, duplicated here so the daemon still runs sanely if the
//! file is missing entirely.

use super::settings::{
    AuthSettings, IdleSettings, IpcSettings, LockSettings, LoggingSettings, PowerSettings,
    SeatSettings, SessionSettings,
};
use std::path::PathBuf;

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            runtime_dir: PathBuf::from("/run/mitos-session"),
            max_sessions_per_user: 4,
            default_session_type: "wayland".to_string(),
            compositor_binary: "/usr/bin/mitos-gui".to_string(),
        }
    }
}

impl Default for SeatSettings {
    fn default() -> Self {
        Self {
            default_seat: "seat0".to_string(),
        }
    }
}

impl Default for LockSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            lock_on_idle: true,
            lock_on_suspend: true,
            grace_period_secs: 5,
            max_auth_attempts: 3,
            lockout_secs: 30,
        }
    }
}

impl Default for IdleSettings {
    fn default() -> Self {
        Self {
            dim_after_secs: 60,
            lock_after_secs: 300,
            suspend_after_secs: 600,
        }
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            pam_service: "mitos-session".to_string(),
            allow_empty_password: false,
        }
    }
}

impl Default for PowerSettings {
    fn default() -> Self {
        Self {
            allow_suspend: true,
            allow_reboot: true,
            allow_poweroff: true,
            suspend_inhibit_grace_secs: 2,
        }
    }
}

impl Default for IpcSettings {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/mitos-session/session.sock"),
            socket_mode: 0o660,
        }
    }
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json: false,
            audit_log_path: Some(PathBuf::from("/var/log/mitos/session-audit.log")),
        }
    }
}
