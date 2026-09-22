use crate::authentication::AuthOutcome;
use crate::elevation::{ElevationAction, ElevationRequestId, ElevationResponse};
use crate::lock::{InhibitMode, InhibitWhat, LockReason};
use crate::session::{SessionId, SessionType};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use zeroize::ZeroizingString;

// --- NEW DATA STRUCTURES ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub user_name: String,
    pub display_name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccessibilitySettings {
    pub screen_reader: bool,
    pub high_contrast: bool,
    pub large_text: bool,
    pub reduce_motion: bool,
    pub keyboard_navigation: bool,
    pub sticky_keys: bool,
    pub slow_keys: bool,
    pub on_screen_keyboard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPolicy {
    pub redact_bodies: bool,
    pub suppress_banners: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStatus {
    pub network_online: bool,
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Permission {
    ScreenCapture,
    RawInput,
    GlobalShortcuts,
}
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
    // CHANGE: String -> ZeroizingString
    Unlock {
        session_id: SessionId,
        user_name: String,
        password: ZeroizingString,
    },

    CheckPermission {
        session_id: SessionId,
        app_uid: u32,
        permission: Permission,
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
    /// Ask mitos-session to verify `session_id`'s logged-in user
    /// before a privileged action proceeds -- see the `elevation`
    /// module. Sent by mitos-service (the permission-policy daemon)
    /// or root; `policy::authorize` rejects anyone else.
    ///
    /// The reply is deferred: unlike every other `Request`, the
    /// `Response` to this one doesn't arrive until the prompt is
    /// resolved by a matching `RespondElevation`, by the session
    /// ending, or by timing out (`[elevation].prompt_timeout_secs`) --
    /// so a caller using the simple one-shot `IpcClient` should expect
    /// this call to block for as long as it takes a human to respond,
    /// not treat a slow reply as a hung connection.
    RequestElevation {
        session_id: SessionId,
        action: ElevationAction,
    },
    /// What the user did with an open elevation prompt. Sent by the
    /// session's registered compositor, or root -- `policy::authorize`
    /// checks the connection this arrives on against exactly which
    /// compositor `request_id`'s prompt was shown to, and rejects
    /// anyone else the same way it would reject a stranger guessing
    /// `request_id`s at random.
    RespondElevation {
        request_id: ElevationRequestId,
        response: ElevationResponse,
    },

    // Phase 3: Login/Greeter queries
    ListAccounts,
    ListSessionTypes,
    GetSystemStatus,

    // Phase 3: Accessibility
    SetAccessibilitySettings(AccessibilitySettings),
    GetAccessibilitySettings,
}

/// Direct reply to exactly one `Request` -- usually sent the moment
/// that request is handled, but not always: `RequestElevation`'s
/// reply is deferred (see its doc comment), and `AuthResult` is what
/// eventually arrives for it, exactly as it would for `Unlock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Sessions(Vec<SessionInfo>),
    Session(SessionInfo),
    AuthResult(AuthOutcome),
    InhibitGranted { inhibit_id: u64 },
    Inhibitors(Vec<InhibitorInfo>),
    Error(String),
    PermissionGranted,
    PermissionDenied(String),
    Accounts(Vec<Account>),
    SessionTypes(Vec<String>),
    SystemStatus(SystemStatus),
    AccessibilitySettings(AccessibilitySettings),
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
    /// Draw an elevation prompt. Only ever sent to `session_id`'s
    /// registered compositor at the moment the request was opened --
    /// `action` is display metadata only, see `elevation::ElevationAction`.
    ShowElevationPrompt {
        request_id: ElevationRequestId,
        session_id: SessionId,
        action: ElevationAction,
    },
    /// One attempt against an open elevation prompt was checked --
    /// mirrors `AuthFeedback`. On a plain `Failure` the prompt stays
    /// open for another try; every other outcome is followed by
    /// `HideElevationPrompt` for the same `request_id`.
    ElevationFeedback {
        request_id: ElevationRequestId,
        outcome: AuthOutcome,
    },
    /// `request_id`'s prompt is over -- dismiss it. Sent whether it
    /// resolved successfully, was cancelled, timed out, or its session
    /// ended before anyone answered.
    HideElevationPrompt {
        request_id: ElevationRequestId,
    },

    // Phase 3: State synchronization
    SessionStateChanged {
        session_id: SessionId,
        state: String, // e.g., "Active", "Locked"
        locked: bool,
    },
    NotificationPolicyChanged(NotificationPolicy),
    SystemStatusChanged(SystemStatus),
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
