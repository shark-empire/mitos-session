use crate::authentication::AuthPolicy;
use crate::config::{AuthSettings, ElevationSettings, LockSettings};
use crate::elevation::ElevationManager;
use crate::errors::{Result, SessionError};
use crate::ipc::{ConnId, PeerCred, Permission, Request};
use crate::session::{SessionId, SessionManager};
use std::time::Duration;

/// Central "is this peer allowed to make this request" check, based on
/// `SO_PEERCRED` identity (see `ipc::permissions`) -- there is no
/// separate login step on the socket itself. Root can do anything;
/// everyone else can only act on sessions they own. A handful of
/// system-wide operations (suspend/reboot/poweroff, inhibitors) are
/// allowed for any local user by design, matching logind's default
/// behaviour without a polkit-equivalent in front of it -- see
/// README's roadmap for adding one.
///
/// Two requests need more than a peer's uid and `sessions` to decide,
/// which is why this also takes `conn_id` and `elevation`:
/// `RequestElevation` is gated on one *specific configured account*
/// (mitos-service), not on owning any session, and `RespondElevation`
/// is gated on being the exact connection mitos-session already
/// pushed that specific prompt to -- see `docs/security.md`'s
/// Elevation section for why both are stricter than the
/// session-ownership check everything else here uses.
pub fn authorize(
    peer: &PeerCred,
    conn_id: ConnId,
    request: &Request,
    sessions: &SessionManager,
    elevation: &ElevationManager,
) -> Result<(), SessionError> {
    
    // --- PERMISSION GATE CHECK (Applies to EVERYONE, including root) ---
    // We check this first because if the session is locked, not even 
    // root (asking on behalf of an app) should be granted raw input 
    // or screen capture.
    if let Request::CheckPermission {
        session_id,
        app_uid,
        permission,
    } = request
    {
        let ctx = sessions.get(*session_id)?;

        // GATE 1: If session is locked, NO app gets raw input or screen capture.
        if ctx.state.is_locked() {
            match permission {
                Permission::ScreenCapture
                | Permission::RawInput
                | Permission::GlobalShortcuts => {
                    return Err(SessionError::PermissionDenied(
                        "session is locked; sensitive permissions revoked".into(),
                    ));
                }
                _ => {}
            }
        }

        // GATE 2: App UID must match the peer UID to prevent spoofing.
        // (Root is exempt for system services like mitos-service asking on behalf of a user app)
        if peer.uid != 0 && peer.uid != *app_uid {
            return Err(SessionError::PermissionDenied(
                "peer UID does not match requested app UID".into(),
            ));
        }

        // If it passed the gates, it's authorized.
        // Future: Check persistent grant database here before returning Ok(())
        return Ok(());
    }

    // --- BLANKET ROOT BYPASS ---
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
            let self_uid = crate::user::User::by_name(user_name)
                .map(|u| u.uid.as_raw())
                .ok();
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

        // Only the configured elevation-requesting service may open a
        // prompt -- never a session owner acting on their own behalf,
        // and never "any locally authenticated user" the way the
        // block above is. Letting arbitrary callers trigger a system
        // password prompt with caller-supplied display text is
        // exactly the phishing vector this restricts against.
        Request::RequestElevation { .. } => {
            if elevation.is_authorized_requester(peer.uid) {
                Ok(())
            } else {
                Err(SessionError::PermissionDenied(
                    "only the configured elevation service may open an elevation prompt".into(),
                ))
            }
        }

        // Only the exact connection mitos-session is expecting an
        // answer from -- the registered compositor for that specific
        // pending request's session -- may resolve it. An unknown id
        // and a wrong-connection attempt produce the identical error
        // on purpose: telling them apart would let a caller probe for
        // which request ids currently exist.
        Request::RespondElevation { request_id, .. } => {
            match elevation.compositor_conn_for(*request_id) {
                Some(expected) if expected == conn_id => Ok(()),
                _ => Err(SessionError::UnknownElevationRequest(*request_id)),
            }
        }

        // Catch-all for any future Request variants added to the IPC protocol
        // that haven't been explicitly whitelisted here yet.
        _ => Err(SessionError::PermissionDenied(
            "unauthorized request variant".into(),
        )),
    }
}

/// Build the `AuthPolicy` used by `authentication::check` for
/// lock-screen unlock attempts. Attempt and lockout limits live under
/// `[lock]` in config (they're user-facing lock-screen behavior) even
/// though the policy object itself belongs to `authentication`.
pub fn auth_policy(auth: &AuthSettings, lock: &LockSettings) -> AuthPolicy {
    AuthPolicy::new(auth, lock)
}

/// Build the `AuthPolicy` elevation credential checks run under.
/// Deliberately separate from `auth_policy` above -- a wrong
/// lock-screen guess and a wrong elevation-prompt guess are different
/// events against different thresholds by default, even though they
/// check the same account's password (see
/// `config::ElevationSettings::pam_service`'s doc comment).
pub fn elevation_auth_policy(elevation: &ElevationSettings) -> AuthPolicy {
    AuthPolicy {
        pam_service: elevation.pam_service.clone(),
        allow_empty_password: elevation.allow_empty_password,
        max_attempts: elevation.max_attempts,
        lockout: Duration::from_secs(elevation.lockout_secs),
    }
}
