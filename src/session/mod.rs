//! The `Session` domain model: what a session is, the state machine it
//! moves through from login to logout, the environment built for it,
//! and the `SessionManager` registry that owns all of the above.

mod context;
mod environment;
mod lifecycle;
mod manager;
#[allow(clippy::module_inception)]
mod session;
mod state;

pub use context::SessionContext;
pub use environment::Environment;
pub use manager::SessionManager;
pub use session::{Session, SessionId, SessionType};
pub use state::SessionState;
