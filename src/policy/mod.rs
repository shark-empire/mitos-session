//! Turns on-disk config into the runtime policy objects each subsystem
//! consumes, and centralizes the "is this peer allowed to do this"
//! authorization decision so it lives in exactly one place rather than
//! being re-derived at every IPC call site.

mod lock_policy;
mod security_policy;
mod session_policy;

pub use lock_policy::LockChainPolicy;
pub use security_policy::{auth_policy, authorize};
pub use session_policy::SessionPolicy;
