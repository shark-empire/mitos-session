use super::{DirectBackend, SystemPowerBackend};
use crate::errors::{Result, SessionError};
use crate::ipc::{ConnRegistry, Event};
use crate::lock::{InhibitWhat, LockManager, LockPolicy};
use crate::session::SessionManager;
use std::thread;
use std::time::Duration;

/// Suspend the machine: refuse outright if a `Block` inhibitor is
/// held, give `Delay` inhibitors `grace` to finish up, lock every
/// session first if `lock_on_suspend` is set and tell each registered
/// compositor to prepare, then hand off to the kernel.
pub fn suspend(
    sessions: &SessionManager,
    locks: &mut LockManager,
    lock_policy: &LockPolicy,
    registry: &ConnRegistry,
    grace: Duration,
) -> Result<()> {
    if locks.inhibitors.blocks(InhibitWhat::Suspend) {
        return Err(SessionError::PermissionDenied(
            "an application is inhibiting suspend".into(),
        ));
    }

    let delayers: Vec<String> = locks.inhibitors.delays(InhibitWhat::Suspend).map(|i| i.who.clone()).collect();
    if !delayers.is_empty() {
        tracing::info!(?delayers, ?grace, "waiting for suspend-delay inhibitors");
        thread::sleep(grace);
    }

    for ctx in sessions.list() {
        if lock_policy.lock_on_suspend {
            let _ = locks.lock(ctx.id(), lock_policy);
        }
        if let Some(conn_id) = ctx.compositor_conn {
            registry.send_event(conn_id, Event::PrepareForSleep);
        }
    }

    DirectBackend.suspend()
}
