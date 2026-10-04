//! Suspend/resume/shutdown/reboot orchestration: checking inhibitors,
//! giving delay-mode inhibitors a grace period, telling sessions to
//! lock and their compositors to prepare, and then performing the
//! actual system transition.
//!
//! The transition itself is a thin `SystemPowerBackend` trait with three
//! implementations, selected by `[power].backend` in `session.toml` (see
//! `PowerBackendKind::make`):
//!
//! - **`mitos_power`** (`MitosPowerBackend`, in `mitos_power.rs`) calls
//!   mitos-power's `Suspend`/`Reboot`/`PowerOff` over its own IPC socket
//!   instead of touching the kernel here. **This is the default.**
//!   mitos-power is the component the wider MITOS spec names as the
//!   sole owner of suspend/hibernate/shutdown/reboot; having mitos-session
//!   *also* write to `/sys/power/state` and call `reboot(2)` directly
//!   (which is what this module used to always do) meant two daemons
//!   could independently trigger a kernel-level power transition, with
//!   only mitos-session's path locking sessions and notifying
//!   compositors first. Routing through mitos-power closes that gap:
//!   the kernel call happens in exactly one place, and this module's own
//!   inhibitor-check/lock/notify sequence (`suspend.rs`/`shutdown.rs`/
//!   `reboot.rs`, unchanged below) still runs before it, same as always.
//! - **`direct`** (`DirectBackend`, below) goes straight to `reboot(2)` /
//!   `/sys/power/state` itself. Kept for a mitos-power-less boot --
//!   genuinely minimal, or while developing this project standalone.
//! - **`supervised`** (`SupervisedBackend`, in `supervised.rs`) signals
//!   PID 1 so `mitos-services` stops every supervised service in
//!   dependency order first. mitos-power's own `shutdown.backend` config
//!   now offers this same choice for when mitos-power itself performs
//!   the transition -- which, under the new default, is every time.

mod mitos_power;
mod reboot;
mod resume;
mod shutdown;
mod supervised;
mod suspend;

pub use mitos_power::MitosPowerBackend;
pub use reboot::reboot;
pub use resume::on_resume;
pub use shutdown::poweroff;
pub use supervised::SupervisedBackend;
pub use suspend::suspend;

use crate::errors::Result;

pub trait SystemPowerBackend {
    fn suspend(&self) -> Result<()>;
    fn poweroff(&self) -> Result<()>;
    fn reboot(&self) -> Result<()>;
}

/// `[power].backend` in `session.toml`. Defaults to `MitosPower` -- see
/// the module doc comment above for why routing the actual kernel
/// transition through mitos-power, rather than duplicating it here, is
/// now the recommended choice whenever mitos-power is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerBackendKind {
    #[default]
    #[serde(rename = "mitos_power")]
    MitosPower,
    Direct,
    Supervised,
}

impl PowerBackendKind {
    pub fn make(&self, mitos_power_socket: &std::path::Path) -> Box<dyn SystemPowerBackend> {
        match self {
            PowerBackendKind::MitosPower => {
                Box::new(MitosPowerBackend::new(mitos_power_socket.to_path_buf()))
            }
            PowerBackendKind::Direct => Box::new(DirectBackend),
            PowerBackendKind::Supervised => Box::new(SupervisedBackend),
        }
    }
}

/// Talks to the kernel directly. See the module doc comment above for
/// why `MitosPowerBackend` is preferred whenever mitos-power is present
/// -- this remains available for a mitos-power-less boot.
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
