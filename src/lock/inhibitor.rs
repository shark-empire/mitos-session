use crate::errors::{Result, SessionError};
use std::collections::HashMap;

/// What an inhibitor is protecting against. Global, not per-session --
/// "don't suspend while I'm burning this disc" applies system-wide,
/// the same as systemd-logind's inhibitor locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum InhibitWhat {
    Idle,
    Lock,
    Suspend,
    Shutdown,
}

/// `Block` prevents the action outright until released; `Delay` lets
/// it proceed but only after the holder has had a chance to act (e.g.
/// flush state to disk before suspend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InhibitMode {
    Block,
    Delay,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Inhibitor {
    pub id: u64,
    pub what: InhibitWhat,
    pub who: String,
    pub why: String,
    pub mode: InhibitMode,
}

#[derive(Debug, Default)]
pub struct InhibitorRegistry {
    inhibitors: HashMap<u64, Inhibitor>,
    next_id: u64,
}

impl InhibitorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(
        &mut self,
        what: InhibitWhat,
        who: impl Into<String>,
        why: impl Into<String>,
        mode: InhibitMode,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.inhibitors.insert(
            id,
            Inhibitor {
                id,
                what,
                who: who.into(),
                why: why.into(),
                mode,
            },
        );
        id
    }

    pub fn remove(&mut self, id: u64) -> Result<()> {
        self.inhibitors
            .remove(&id)
            .map(|_| ())
            .ok_or(SessionError::UnknownInhibitor(id))
    }

    pub fn blocks(&self, what: InhibitWhat) -> bool {
        self.inhibitors
            .values()
            .any(|i| i.what == what && i.mode == InhibitMode::Block)
    }

    pub fn delays(&self, what: InhibitWhat) -> impl Iterator<Item = &Inhibitor> {
        self.inhibitors
            .values()
            .filter(move |i| i.what == what && i.mode == InhibitMode::Delay)
    }

    pub fn list(&self) -> impl Iterator<Item = &Inhibitor> {
        self.inhibitors.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_is_visible_to_blocks_query() {
        let mut reg = InhibitorRegistry::new();
        assert!(!reg.blocks(InhibitWhat::Suspend));
        let id = reg.add(
            InhibitWhat::Suspend,
            "backup-daemon",
            "running backup",
            InhibitMode::Block,
        );
        assert!(reg.blocks(InhibitWhat::Suspend));
        reg.remove(id).unwrap();
        assert!(!reg.blocks(InhibitWhat::Suspend));
    }

    #[test]
    fn removing_unknown_id_errors() {
        let mut reg = InhibitorRegistry::new();
        assert!(reg.remove(999).is_err());
    }
}
