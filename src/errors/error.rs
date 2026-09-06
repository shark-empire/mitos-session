use std::io;
use std::sync::PoisonError;
use thiserror::Error;

/// Crate-wide result alias. Every fallible public function in
/// mitos-session returns this rather than a bespoke per-module error.
pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("no such user: {0}")]
    UnknownUser(String),

    #[error("no such seat: {0}")]
    UnknownSeat(String),

    #[error("no such session: {0}")]
    UnknownSession(u32),

    #[error("no such inhibitor: {0}")]
    UnknownInhibitor(u64),

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("account is temporarily locked out ({0}s remaining)")]
    LockedOut(u64),

    #[error("PAM error: {0}")]
    Pam(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("ipc protocol error: {0}")]
    Protocol(String),

    #[error("ipc peer disconnected")]
    Disconnected,

    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("operation not supported: {0}")]
    Unsupported(String),

    #[error("a background operation was cancelled")]
    Cancelled,

    #[error("internal lock was poisoned: {0}")]
    Poisoned(String),
}

impl<T> From<PoisonError<T>> for SessionError {
    fn from(e: PoisonError<T>) -> Self {
        SessionError::Poisoned(e.to_string())
    }
}

impl From<bincode::Error> for SessionError {
    fn from(e: bincode::Error) -> Self {
        SessionError::Protocol(e.to_string())
    }
}

impl From<toml::de::Error> for SessionError {
    fn from(e: toml::de::Error) -> Self {
        SessionError::Config(e.to_string())
    }
}

impl From<nix::Error> for SessionError {
    fn from(e: nix::Error) -> Self {
        SessionError::Io(io::Error::from(e))
    }
}
