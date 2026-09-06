use super::{DirectBackend, SystemPowerBackend};
use crate::errors::{Result, SessionError};
use crate::lock::{InhibitWhat, LockManager};
use crate::session::SessionManager;
use std::thread;
use std::time::Duration;

/// Power the machine off: refuse if inhibited, give delay-inhibitors
/// their grace period, cleanly terminate every session, then hand off
/// to the kernel. Session termination is best-effort -- a session
/// whose compositor won't die is killed outright rather than blocking
/// shutdown indefinitely (see `session::lifecycle::end`).
pub fn poweroff(sessions: &mut SessionManager, locks: &LockManager, grace: Duration) -> Result<()> {
    if locks.inhibitors.blocks(InhibitWhat::Shutdown) {
        return Err(SessionError::PermissionDenied(
            "an application is inhibiting shutdown".into(),
        ));
    }

    let delayers: Vec<String> = locks
        .inhibitors
        .delays(InhibitWhat::Shutdown)
        .map(|i| i.who.clone())
        .collect();
    if !delayers.is_empty() {
        tracing::info!(?delayers, ?grace, "waiting for shutdown-delay inhibitors");
        thread::sleep(grace);
    }

    let ids: Vec<_> = sessions.list().map(|c| c.id()).collect();
    for id in ids {
        let _ = sessions.terminate_session(id);
    }

    DirectBackend.poweroff()
}
