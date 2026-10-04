use serde::{Deserialize, Serialize};

/// What came back from an authentication attempt -- sent to the
/// compositor over IPC so it can show the right feedback, for either
/// of the two flows that produce one of these: unlock attempts
/// (`ipc::messages::Event::AuthFeedback`) and elevation prompts
/// (`ipc::messages::Event::ElevationFeedback`, and the final
/// `ipc::messages::Response::AuthResult` an elevation caller gets
/// back).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthOutcome {
    Success,
    Failure { attempts_remaining: u32 },
    LockedOut { retry_after_secs: u64 },
    Error(String),
    /// Whoever was answering explicitly declined rather than getting
    /// the credential wrong -- distinct from `Failure` so it never
    /// counts against the attempt/lockout budget. Currently only ever
    /// produced by `elevation::ElevationManager::cancel`; lock/unlock
    /// has no "cancel" concept of its own, only a password box that's
    /// either filled in correctly or isn't.
    ///
    /// Added after the other four, and MUST stay last: this type rides
    /// the wire (`bincode` encodes enum variants by ordinal position),
    /// so inserting a variant anywhere else would silently reorder
    /// every variant after it for any peer still running older code.
    Cancelled,
}
