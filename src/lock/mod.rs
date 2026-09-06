//! Screen-lock state machine and policy. This module decides *whether*
//! a session is locked and *whether* an unlock attempt succeeds -- it
//! never draws anything. The actual lock-screen UI is mitos-gui's job;
//! this module drives it by pushing `ipc::messages::Event::ShowLockScreen`
//! / `HideLockScreen` / `AuthFeedback` to the registered compositor.

mod inhibitor;
#[allow(clippy::module_inception)]
mod lock;
mod policy;
mod state;
mod timeout;

pub use inhibitor::{InhibitMode, InhibitWhat, Inhibitor, InhibitorRegistry};
pub use lock::{LockManager, LockReason};
pub use policy::LockPolicy;
pub use state::LockState;
pub use timeout::LockTimeouts;
