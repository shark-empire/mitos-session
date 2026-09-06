//! General `tracing` setup, kept separate from `audit` -- security
//! relevant events (logins, lock/unlock, privileged IPC requests,
//! power transitions) go to a dedicated audit log even if someone
//! dials general logging down to `error`.

mod audit;

pub use audit::{audit_log, configure_audit_log, AuditEvent};

use crate::config::LoggingSettings;
use tracing_subscriber::EnvFilter;

/// Initialize the general-purpose tracing subscriber. `RUST_LOG`, if
/// set, always wins over `session.toml`'s `logging.level` -- handy for
/// a one-off debug run without editing config.
pub fn init(settings: &LoggingSettings) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(settings.level.clone()));
    let subscriber = tracing_subscriber::fmt().with_env_filter(filter);
    if settings.json {
        subscriber.json().init();
    } else {
        subscriber.init();
    }
}
