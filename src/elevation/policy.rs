use crate::config::ElevationSettings;
use std::time::Duration;

/// Runtime elevation policy, built once from config. Deliberately
/// separate from the `authentication::AuthPolicy` elevation credential
/// checks run under (see `policy::elevation_auth_policy`) -- this
/// struct governs the prompt/session bookkeeping around a check,
/// not the credential check itself.
#[derive(Debug, Clone, Copy)]
pub struct ElevationPolicy {
    pub enabled: bool,
    /// How long an open prompt waits for `RespondElevation` before
    /// it's abandoned as timed out.
    pub prompt_timeout: Duration,
    /// Cap on concurrently outstanding prompts per session, so a
    /// hostile or malfunctioning caller can't flood one session with
    /// prompts.
    pub max_pending_per_session: usize,
}

impl From<&ElevationSettings> for ElevationPolicy {
    fn from(s: &ElevationSettings) -> Self {
        Self {
            enabled: s.enabled,
            prompt_timeout: Duration::from_secs(s.prompt_timeout_secs),
            max_pending_per_session: s.max_pending_per_session as usize,
        }
    }
}
