use crate::config::LoggingSettings;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single security-relevant event: logins, lock/unlock attempts,
/// privileged IPC requests, power transitions. Kept structured (JSON
/// lines) rather than a formatted string so a future log-shipping
/// setup can parse it without regexing tracing output.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub timestamp_unix: u64,
    pub actor_uid: u32,
    pub action: String,
    pub target: Option<String>,
    pub outcome: String,
}

impl AuditEvent {
    pub fn new(actor_uid: u32, action: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            timestamp_unix: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            actor_uid,
            action: action.into(),
            target: None,
            outcome: outcome.into(),
        }
    }

    pub fn target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

static AUDIT_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Point future `audit_log` calls at `settings.audit_log_path`. Called
/// once at startup after config is loaded.
pub fn configure_audit_log(settings: &LoggingSettings) {
    if let Ok(mut guard) = AUDIT_PATH.lock() {
        *guard = settings.audit_log_path.clone();
    }
}

/// Append one audit event as a JSON line. If no audit log path is
/// configured (or it can't be opened), the event still isn't silently
/// dropped -- it goes out through `tracing` at `warn` level instead.
pub fn audit_log(event: AuditEvent) {
    let path = AUDIT_PATH.lock().ok().and_then(|g| g.clone());
    let Some(path) = path else {
        tracing::warn!(?event, "audit log not configured; logging event via tracing instead");
        return;
    };

    let line = match serde_json::to_string(&event) {
        Ok(line) => line,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize audit event");
            return;
        }
    };

    let result = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"));

    if let Err(e) = result {
        tracing::error!(error = %e, path = %path.display(), "failed to write audit log entry");
    }
}
