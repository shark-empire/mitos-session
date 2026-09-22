use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub max_attempts: u32,
    pub base_lockout: Duration,
    pub max_lockout: Duration,
}

impl AuthPolicy {
    /// Calculates the lockout duration based on failed attempts.
    /// Uses exponential backoff with deterministic jitter to prevent timing attacks.
    pub fn lockout_duration(&self, failures: u32, session_id: &str) -> Duration {
        if failures < self.max_attempts {
            return Duration::ZERO;
        }

        // Exponential backoff: base * 2^(failures - max_attempts)
        let exponent = failures.saturating_sub(self.max_attempts).min(10);
        let base_ms = self.base_lockout.as_millis() as u64;
        let mut delay_ms = base_ms.saturating_mul(2u64.saturating_pow(exponent));
        
        // Cap at max_lockout
        delay_ms = delay_ms.min(self.max_lockout.as_millis() as u64);

        // Add jitter (up to 20% of the delay)
        let jitter_max = delay_ms / 5;
        
        let mut hasher = DefaultHasher::new();
        session_id.hash(&mut hasher);
        failures.hash(&mut hasher);
        let hash = hasher.finish();
        
        let jitter = (hash % (jitter_max + 1)) as u64;

        Duration::from_millis(delay_ms + jitter)
    }
}
