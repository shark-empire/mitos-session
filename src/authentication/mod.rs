//! PAM-backed credential checking for unlock attempts, plus the
//! attempt-count/lockout policy layered on top of it. `lock::lock`
//! calls into `check` and never talks to PAM directly.

mod authenticator;
mod policy;
mod request;
mod result;

pub use authenticator::{check, Authenticator, PamAuthenticator};
pub use policy::AuthPolicy;
pub use request::AuthRequest;
pub use result::AuthOutcome;
