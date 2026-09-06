use super::{DirectBackend, SystemPowerBackend};
use crate::errors::{Result, SessionError};
use crate::lock::{InhibitWhat, LockManager};
use crate::session::SessionManager;
use std::thread;
use std::time::Duration;

/// Same shape as `shutdown::poweroff`, ending in `reboot(2)` with
/// `RB_AUTOBOOT` instead of `RB_POWER_OFF`.
pub fn reboot(sessions: &mut SessionManager, locks: &LockManager, grace: Duration) -> Result<()> {
    if locks.inhibitors.blocks(InhibitWhat::Shutdown) {
        return Err(SessionError::PermissionDenied(
            "an application is inhibiting shutdown/reboot".into(),
        ));
    }

    let delayers: Vec<String> = locks
        .inhibitors
        .delays(InhibitWhat::Shutdown)
        .map(|i| i.who.clone())
        .collect();
    if !delayers.is_empty() {
        thread::sleep(grace);
    }

    let ids: Vec<_> = sessions.list().map(|c| c.id()).collect();
    for id in ids {
        let _ = sessions.terminate_session(id);
    }

    DirectBackend.reboot()
}
