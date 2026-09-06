use crate::authentication::AuthPolicy;
use crate::config::{AuthSettings, LockSettings};
use crate::errors::{Result, SessionError};
use crate::ipc::{PeerCred, Request};
use crate::session::{SessionId, SessionManager};

/// Central "is this peer allowed to make this request" check, based on
/// `SO_PEERCRED` identity (see `ipc::permissions`) -- there is no
/// separate login step on the socket itself. Root can do anything;
/// everyone else can only act on sessions they own. A handful of
/// system-wide operations (suspend/reboot/poweroff, inhibitors) are
/// allowed for any local user by design, matching logind's default
/// behaviour without a polkit-equivalent in front of it -- see
/// README's roadmap for adding one.
pub fn authorize(peer: &PeerCred, request: &Request, sessions: &SessionManager) -> Result<()> {
    if peer.uid == 0 {
        return Ok(());
    }

    let owns = |session_id: SessionId| -> Result<()> {
        match sessions.owner_uid(session_id) {
            Ok(uid) if uid == peer.uid => Ok(()),
            Ok(_) => Err(SessionError::PermissionDenied(format!(
                "uid {} does not own session {session_id}",
                peer.uid
            ))),
            Err(e) => Err(e),
        }
    };

    match request {
        // Starting a session for someone else requires root (the
        // greeter/login prompt normally runs as root); starting one
        // for yourself is always fine.
        Request::CreateSession { user_name, .. } => {
            let self_uid = crate::user::User::by_name(user_name).map(|u| u.uid.as_raw()).ok();
            if self_uid == Some(peer.uid) {
                Ok(())
            } else {
                Err(SessionError::PermissionDenied(
                    "only root may start a session for another user".into(),
                ))
            }
        }
        Request::TerminateSession { session_id }
        | Request::RegisterCompositor { session_id }
        | Request::SessionStatus { session_id }
        | Request::LockSession { session_id }
        | Request::SwitchSession { session_id, .. } => owns(*session_id),
        Request::Unlock { session_id, .. } => owns(*session_id),

        // Listing sessions/inhibitors, reporting activity, taking out
        // an inhibitor, or asking the system to suspend/reboot/power
        // off is allowed for any locally-authenticated user.
        Request::ListSessions
        | Request::ReportActivity { .. }
        | Request::Inhibit { .. }
        | Request::ReleaseInhibit { .. }
        | Request::ListInhibitors
        | Request::Suspend
        | Request::Reboot
        | Request::PowerOff => Ok(()),
    }
}

/// Build the `AuthPolicy` used by `authentication::check`. Attempt and
/// lockout limits live under `[lock]` in config (they're user-facing
/// lock-screen behavior) even though the policy object itself belongs
/// to `authentication`.
pub fn auth_policy(auth: &AuthSettings, lock: &LockSettings) -> AuthPolicy {
    AuthPolicy::new(auth, lock)
}
