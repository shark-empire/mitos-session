/// A session's lock status. Distinct from (and finer-grained than)
/// `session::SessionState::Locked` -- `SessionState` is the coarse
/// lifecycle stage, this tracks the lock/auth sub-state within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locked,
    /// Locked, and additionally refusing new auth attempts until the
    /// lockout timer (`lock::timeout::LockTimeouts`) expires.
    LockedOut,
}

impl Default for LockState {
    fn default() -> Self {
        LockState::Unlocked
    }
}
