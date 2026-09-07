//! The shape of `session.toml`. Every struct here derives `Default`
//! (implemented in `defaults.rs`) so that a config file only needs to
//! mention the keys it wants to override.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    pub session: SessionSettings,
    pub seat: SeatSettings,
    pub lock: LockSettings,
    pub idle: IdleSettings,
    pub authentication: AuthSettings,
    pub power: PowerSettings,
    pub ipc: IpcSettings,
    pub logging: LoggingSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SessionSettings {
    pub runtime_dir: PathBuf,
    pub max_sessions_per_user: u32,
    pub default_session_type: String,
    /// Compositor binary spawned for non-tty sessions. Points at
    /// mitos-gui by default.
    pub compositor_binary: String,
    /// How many times to relaunch a session's compositor after it
    /// exits unexpectedly before giving up and falling back to a
    /// plain terminal. See `launcher::decide_restart`.
    pub max_compositor_restarts: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SeatSettings {
    pub default_seat: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LockSettings {
    pub enabled: bool,
    pub lock_on_idle: bool,
    pub lock_on_suspend: bool,
    pub grace_period_secs: u64,
    pub max_auth_attempts: u32,
    pub lockout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IdleSettings {
    pub dim_after_secs: u64,
    pub lock_after_secs: u64,
    pub suspend_after_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthSettings {
    pub pam_service: String,
    pub allow_empty_password: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PowerSettings {
    pub allow_suspend: bool,
    pub allow_reboot: bool,
    pub allow_poweroff: bool,
    pub suspend_inhibit_grace_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IpcSettings {
    pub socket_path: PathBuf,
    pub socket_mode: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LoggingSettings {
    pub level: String,
    pub json: bool,
    pub audit_log_path: Option<PathBuf>,
}
