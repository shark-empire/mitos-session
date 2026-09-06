//! Idle detection: turning "how long since any input happened on this
//! seat" into dim/lock/suspend events. Decoupled on purpose from
//! `lock` and `power` -- this module only ever says "seat0 just
//! crossed the lock threshold," it never locks anything itself. The
//! daemon's main loop is what wires that event into
//! `lock::LockManager::request_lock`.

mod detector;
mod policy;
mod tracker;

pub use detector::IdleDetector;
pub use policy::{IdlePolicy, IdleStage};
pub use tracker::IdleTracker;
