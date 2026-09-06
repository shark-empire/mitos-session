use serde::{Deserialize, Serialize};

/// What came back from an authentication attempt -- sent to the
/// compositor over IPC so it can show the right lock-screen feedback
/// (`ipc::messages::Event::AuthFeedback`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthOutcome {
    Success,
    Failure { attempts_remaining: u32 },
    LockedOut { retry_after_secs: u64 },
    Error(String),
}
