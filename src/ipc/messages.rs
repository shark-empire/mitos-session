use crate::authentication::AuthOutcome;
use crate::lock::{InhibitMode, InhibitWhat, LockReason};
use crate::session::{SessionId, SessionType};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Messages a client -- mitos-gui, mitos-sessionctl, or any other
/// authorized peer -- sends to mitos-session. Authorization for each
/// variant is decided by `policy::security_policy`, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Start a new session for `user_name`. Sent by whatever runs the
    /// login prompt after it has already verified the user's identity
    /// -- mitos-session does not re-authenticate here, it trusts the
    /// caller's credentials (root, or the account itself) the same
    /// way `authentication` trusts PAM for unlock. `seat_id` and
    /// `session_type` default to config's `[seat].default_seat` and
    /// `[session].default_session_type` when omitted.
    CreateSession {
        user_name: String,
        seat_id: Option<String>,
        session_type: Option<String>,
    },
    /// End a session outright (logout), as opposed to `LockSession`.
    TerminateSession {
        session_id: SessionId,
    },
    /// Register this connection as the compositor responsible for a
    /// session. Required before the connection will receive `Event`s
    /// for that session (lock screen show/hide, dim, etc).
    RegisterCompositor {
        session_id: SessionId,
    },
    ListSessions,
    SessionStatus {
        session_id: SessionId,
    },
    LockSession {
        session_id: SessionId,
    },
    Unlock {
        session_id: SessionId,
        user_name: String,
        password: String,
    },
    /// Coalesced "input happened" ping -- resets the idle timer for a
    /// seat. Sent by the compositor, never carries raw input events.
    ReportActivity {
        seat_id: String,
    },
    SwitchSession {
        seat_id: String,
        session_id: SessionId,
    },
    Inhibit {
        what: InhibitWhat,
        who: String,
        why: String,
        mode: InhibitMode,
    },
    ReleaseInhibit {
        inhibit_id: u64,
    },
    ListInhibitors,
    Suspend,
    Reboot,
    PowerOff,
}

/// Direct reply to exactly one `Request`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Sessions(Vec<SessionInfo>),
    Session(SessionInfo),
    AuthResult(AuthOutcome),
    InhibitGranted { inhibit_id: u64 },
    Inhibitors(Vec<InhibitorInfo>),
    Error(String),
}

/// Messages mitos-session pushes to a registered compositor without
/// being asked. These are what actually drive the lock-screen UI --
/// mitos-gui reacts to `ShowLockScreen`/`HideLockScreen`/`AuthFeedback`
/// but never decides on its own to lock or unlock anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    ShowLockScreen {
        session_id: SessionId,
        reason: LockReason,
    },
    HideLockScreen {
        session_id: SessionId,
    },
    AuthFeedback {
        session_id: SessionId,
        outcome: AuthOutcome,
    },
    Dim {
        seat_id: String,
    },
    Undim {
        seat_id: String,
    },
    PrepareForSleep,
    ResumedFromSleep,
    SessionActivated {
        seat_id: String,
        session_id: SessionId,
    },
}

/// Everything the server ever writes to a connection: either a direct
/// `Response` to something the client asked, or an unsolicited
/// `Event`. Clients only ever *write* `Request` and only ever *read*
/// `Message`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Response(Response),
    Event(Event),
}

/// Serializable snapshot of a session, as returned by `ListSessions`
/// and `SessionStatus`. Deliberately narrower than the manager's
/// internal `SessionContext` -- no environment variables, no process
/// handles, nothing a client shouldn't see.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub uid: u32,
    pub user_name: String,
    pub seat_id: String,
    pub session_type: SessionType,
    pub state: String,
    pub locked: bool,
    pub created_at: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InhibitorInfo {
    pub id: u64,
    pub what: InhibitWhat,
    pub who: String,
    pub why: String,
    pub mode: InhibitMode,
}
