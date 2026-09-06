use crate::errors::Result;
use crate::ipc::{ConnRegistry, Event};
use crate::session::SessionManager;

/// Run right after the daemon notices the system has resumed from
/// suspend. Sessions were already locked before suspending (if
/// `lock_on_suspend` was set) -- there's nothing to re-lock here, this
/// just tells every registered compositor the display is back so it
/// can redraw / re-check the clock on the lock screen.
pub fn on_resume(sessions: &SessionManager, registry: &ConnRegistry) -> Result<()> {
    for ctx in sessions.list() {
        if let Some(conn_id) = ctx.compositor_conn {
            registry.send_event(conn_id, Event::ResumedFromSleep);
        }
    }
    Ok(())
}
