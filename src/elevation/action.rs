use serde::{Deserialize, Serialize};

/// Coarse, display-only risk tier for an elevation prompt.
/// mitos-session never branches on this -- mitos-service's rulebook
/// already decided the action needs a password before it ever asks;
/// this only tells mitos-gui how to style the prompt (e.g. a red
/// "Critical" badge). Mirrors `lock::LockReason` in spirit: carried
/// through so the UI can react, never inspected by this crate's own
/// logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElevationRisk {
    Elevated,
    Critical,
}

/// Everything mitos-service supplies about *why* it's asking, purely
/// for the compositor to render. mitos-session treats every field here
/// as an opaque, length-bounded label (see `validate`/`MAX_LABEL_LEN`),
/// not something it parses or makes decisions from -- the
/// authorization decision was already made by mitos-service before
/// this ever arrived; mitos-session's only remaining job is proving
/// the logged-in user is really who's typing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElevationAction {
    /// Human-readable name of the app that triggered this, as resolved
    /// by mitos-service (e.g. "Video Editor").
    pub requesting_app: String,
    /// Human-readable description of the action (e.g. "Raw Disk
    /// Access").
    pub description: String,
    pub risk: ElevationRisk,
    /// How long a grant would last if approved, already formatted by
    /// mitos-service for display (e.g. "5 minutes", "This session",
    /// "Once") -- mitos-session doesn't interpret or enforce this, it
    /// never writes the actual grant; see mitos-service's rulebook.
    pub duration_label: String,
}

/// Upper bound on every `ElevationAction` string field, enforced when
/// a `RequestElevation` is received (`Daemon::start_elevation` in
/// `src/main.rs`). Not configurable -- unlike timeouts or attempt
/// limits this isn't a policy knob, it's a sanity backstop against a
/// hostile or malfunctioning caller trying to overflow the
/// compositor's UI. Requests that fail this are rejected outright
/// rather than silently truncated: truncating text a user is about to
/// trust enough to type their password against is exactly the kind of
/// silent change that could flip its meaning without anyone noticing.
pub const MAX_LABEL_LEN: usize = 256;

impl ElevationAction {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.requesting_app.is_empty() {
            return Err("requesting_app must not be empty");
        }
        if self.requesting_app.len() > MAX_LABEL_LEN {
            return Err("requesting_app exceeds the maximum label length");
        }
        if self.description.is_empty() {
            return Err("description must not be empty");
        }
        if self.description.len() > MAX_LABEL_LEN {
            return Err("description exceeds the maximum label length");
        }
        if self.duration_label.len() > MAX_LABEL_LEN {
            return Err("duration_label exceeds the maximum label length");
        }
        Ok(())
    }
}

/// What the user (relayed by mitos-gui) did with an open prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ElevationResponse {
    Password(String),
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action() -> ElevationAction {
        ElevationAction {
            requesting_app: "Video Editor".into(),
            description: "Raw Disk Access".into(),
            risk: ElevationRisk::Critical,
            duration_label: "5 minutes".into(),
        }
    }

    #[test]
    fn ordinary_action_validates() {
        assert!(action().validate().is_ok());
    }

    #[test]
    fn empty_requesting_app_is_rejected() {
        let mut a = action();
        a.requesting_app.clear();
        assert!(a.validate().is_err());
    }

    #[test]
    fn oversized_label_is_rejected_not_truncated() {
        let mut a = action();
        a.description = "x".repeat(MAX_LABEL_LEN + 1);
        assert!(a.validate().is_err());
    }

    #[test]
    fn label_at_exactly_the_limit_is_fine() {
        let mut a = action();
        a.description = "x".repeat(MAX_LABEL_LEN);
        assert!(a.validate().is_ok());
    }
}
