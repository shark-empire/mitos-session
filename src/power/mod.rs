//! Suspend/resume/shutdown/reboot orchestration: checking inhibitors,
//! giving delay-mode inhibitors a grace period, telling sessions to
//! lock and their compositors to prepare, and then performing the
//! actual system transition.
//!
//! The transition itself is a thin `SystemPowerBackend` trait: a
//! minimal standalone MITOS boot can go straight to `reboot(2)` /
//! `/sys/power/state` (`DirectBackend`, below), but a full install
//! with `mitos-services` running should route this through a backend
//! that asks it to stop services in dependency order first instead.
//! That integration point is left as a follow-up -- see README.

mod reboot;
mod resume;
mod shutdown;
mod suspend;

pub use reboot::reboot;
pub use resume::on_resume;
pub use shutdown::poweroff;
pub use suspend::suspend;

use crate::errors::Result;

pub trait SystemPowerBackend {
    fn suspend(&self) -> Result<()>;
    fn poweroff(&self) -> Result<()>;
    fn reboot(&self) -> Result<()>;
}

/// Talks to the kernel directly. Correct for a minimal standalone
/// boot; see the module doc comment above for the integration point a
/// full MITOS install needs instead.
pub struct DirectBackend;

impl SystemPowerBackend for DirectBackend {
    fn suspend(&self) -> Result<()> {
        std::fs::write("/sys/power/state", "mem")?;
        Ok(())
    }

    fn poweroff(&self) -> Result<()> {
        // On success this call does not return -- the machine powers
        // off. `Ok(())` below only ever executes if that somehow isn't
        // true, which would itself indicate something worth a bug
        // report against nix/the kernel, not this code.
        nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_POWER_OFF)?;
        Ok(())
    }

    fn reboot(&self) -> Result<()> {
        nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT)?;
        Ok(())
    }
}
