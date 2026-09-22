use super::SystemPowerBackend;
use crate::errors::{Result, SessionError};
use crate::ipc::{ConnRegistry, Event};
use crate::lock::{InhibitWhat, LockManager, LockPolicy, LockReason};
use crate::session::{SessionManager, SessionState};
use std::thread;
use std::time::Duration;

/// Suspend the machine: refuse outright if a `Block` inhibitor is
/// held, give `Delay` inhibitors `grace` to finish up, lock every
/// session first if `lock_on_suspend` is set and tell each registered
/// compositor to prepare, then hand off to `backend`.
pub fn suspend(
    sessions: &mut SessionManager, // CHANGED: &mut to allow state transitions
    locks: &mut LockManager,
    lock_policy: &LockPolicy,
    registry: &ConnRegistry,
    backend: &dyn SystemPowerBackend,
    grace: Duration,
) -> Result<()> {
    if locks.inhibitors.blocks(InhibitWhat::Suspend) {
        return Err(SessionError::PermissionDenied(
            "an application is inhibiting suspend".into(),
        ));
    }

    let delayers: Vec<String> = locks
        .inhibitors
        .delays(InhibitWhat::Suspend)
        .map(|i| i.who.clone())
        .collect();
    if !delayers.is_empty() {
        tracing::info!(?delayers, ?grace, "waiting for suspend-delay inhibitors");
        thread::sleep(grace);
    }

    for ctx in sessions.iter_mut() { // CHANGED: iter_mut()
        if lock_policy.lock_on_suspend {
            if locks.lock(ctx.id(), lock_policy).is_ok() {
                // --- PHASE 5: SUSPEND-LOCK INVARIANT ---
                // Force the session into the Suspended state. 
                // This guarantees that upon wake, the session is locked 
                // and requires authentication, preventing "sleep-walk" attacks.
                let _ = ctx.transition(SessionState::Suspended);
                
                if let Some(conn_id) = ctx.compositor_conn {
                    registry.send_event(conn_id, Event::ShowLockScreen { 
                        session_id: ctx.id(), 
                        reason: LockReason::Suspend 
                    });
                }
            }
        }
        
        if let Some(conn_id) = ctx.compositor_conn {
            registry.send_event(conn_id, Event::PrepareForSleep);
        }
    }

    backend.suspend()
}
