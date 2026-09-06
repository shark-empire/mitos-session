use super::context::SessionContext;
use super::environment::Environment;
use super::session::{Session, SessionId, SessionType};
use super::state::SessionState;
use crate::config::SessionSettings;
use crate::errors::Result;
use crate::user::{ensure_home_ready, User};
use std::time::SystemTime;

/// Build the full runtime record for a brand-new session: check the
/// home directory, resolve the environment, create `XDG_RUNTIME_DIR`,
/// and hand back a `SessionContext` sitting in `SessionState::Starting`.
///
/// Does not launch anything -- that's `launcher`'s job, driven by
/// `SessionManager` once this returns successfully.
pub fn begin(
    id: SessionId,
    user: User,
    seat_id: &str,
    session_type: SessionType,
    settings: &SessionSettings,
) -> Result<SessionContext> {
    ensure_home_ready(&user)?;

    let (environment, runtime_dir) =
        Environment::for_session(&user, id, session_type, seat_id, &settings.runtime_dir);
    Environment::ensure_runtime_dir(&runtime_dir, &user)?;

    let session = Session {
        id,
        uid: user.uid,
        user_name: user.name.clone(),
        seat_id: seat_id.to_string(),
        session_type,
        vt: None,
        created_at: SystemTime::now(),
    };

    Ok(SessionContext {
        session,
        user,
        state: SessionState::Starting,
        environment,
        compositor_conn: None,
        compositor_process: None,
    })
}

/// Tear down a closing session: kill the compositor process if it's
/// still around and reap it so it doesn't linger as a zombie. Leaves
/// the caller (`SessionManager`) to strip the session out of its
/// registry and tell `SeatManager` to promote whatever was queued
/// behind it.
pub fn end(ctx: &mut SessionContext) -> Result<()> {
    if let Some(mut child) = ctx.compositor_process.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    // Best-effort: a session that's already mid-teardown should still
    // end up `Closed` even if something upstream got the state machine
    // into a slightly unexpected place.
    ctx.state = ctx
        .state
        .transition(SessionState::Closed)
        .unwrap_or(SessionState::Closed);
    Ok(())
}
