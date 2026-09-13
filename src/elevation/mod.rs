//! Handles requests to verify the logged-in user before a privileged
//! action proceeds -- MITOS's "elevation prompt" concept, distinct
//! from (and orthogonal to) the screen-lock flow in `lock`. A session
//! can be fully unlocked and still get an elevation prompt; answering
//! one never changes `session::SessionState` or `lock::LockState`.
//!
//! The flow has three parties, unlike lock/unlock's two:
//! mitos-service (or an equivalent trusted caller) asks
//! `ipc::Request::RequestElevation`, mitos-session relays a
//! `ipc::Event::ShowElevationPrompt` to the session's registered
//! compositor, the compositor collects a password and answers with
//! `ipc::Request::RespondElevation`, and mitos-session checks it and
//! finally replies to whoever originally asked. See
//! `docs/security.md`'s Elevation section for the full trust-boundary
//! reasoning (in particular: why only the configured service uid may
//! open a prompt, and why only the exact compositor connection it was
//! shown to may answer it).
//!
//! mitos-session's job here is narrow and deliberate: prove the
//! credential is right, the same way `authentication` already does
//! for unlock attempts. Whether the *action* itself is safe to allow
//! was already decided by mitos-service before it ever asked -- this
//! module never sees, interprets, or second-guesses that decision, and
//! never writes a permission grant anywhere; that's the rulebook's job
//! in a different repo entirely.

mod action;
mod manager;
mod policy;
mod timeout;

pub use action::{ElevationAction, ElevationResponse, ElevationRisk, MAX_LABEL_LEN};
pub use manager::{
    AbandonedElevation, ElevationManager, ElevationOutcome, ElevationRequestId,
    PendingElevationInfo,
};
pub use policy::ElevationPolicy;
pub use timeout::ElevationTimeouts;
