use crate::errors::Result;
use crate::session::SessionManager;
use std::path::Path;

/// Tear down cleanly on `SignalEvent::Terminate`: terminate every
/// session (which also reaps their compositor processes -- see
/// `session::lifecycle::end`) and remove the IPC socket file, so a
/// stale socket file doesn't confuse the next `mitos-sessionctl`
/// invocation before the daemon comes back up.
pub fn graceful_shutdown(sessions: &mut SessionManager, socket_path: &Path) -> Result<()> {
    let ids: Vec<_> = sessions.list().map(|c| c.id()).collect();
    for id in ids {
        let _ = sessions.terminate_session(id);
    }
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    tracing::info!("graceful shutdown complete");
    Ok(())
}
