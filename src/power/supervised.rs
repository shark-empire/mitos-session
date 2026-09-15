//! `SystemPowerBackend` that requests a transition through mitos-init's
//! relay to mitos-services, rather than calling the kernel directly.
//! See `mod.rs`'s doc comment for when this is the right choice.

use super::{DirectBackend, SystemPowerBackend};
use crate::errors::Result;
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;

/// Requests a transition by signalling PID 1 (mitos-init) -- the same
/// way `reboot`/`poweroff`/`halt` command-line tools do, per
/// mitos-services' own README ("reboot/poweroff/halt/shutdown still
/// signal PID 1 directly ... a kernel/sysvinit convention"). mitos-init
/// relays the signal to mitos-services, which stops every supervised
/// service in dependency order and acknowledges back to mitos-init
/// over a FIFO; mitos-init performs the actual transition once that
/// ack arrives. This backend never talks to mitos-services directly,
/// and never calls the kernel syscall itself for reboot/poweroff -- it
/// only asks PID 1 to start that sequence, the same as any other
/// authorized requester would.
///
/// The signal mapping below is confirmed against mitos-services' own
/// `signals.rs`, which documents it explicitly: "SIGINT means reboot,
/// SIGTERM means power off ... matching the classic sysvinit
/// convention mitos-init's own relay preserves." It is *not*
/// independently confirmed against mitos-init itself -- there's no
/// mitos-init source in this project to check it against yet -- so if
/// that relay's mapping ever changes, this does too.
pub struct SupervisedBackend;

impl SystemPowerBackend for SupervisedBackend {
    /// Suspend never stops a single service -- everything stays
    /// resident in memory, just frozen -- so there's no supervisor
    /// sequence to kick off, and mitos-services' shutdown-family
    /// signals don't cover it anyway (see its `signals.rs`).
    /// Identical to `DirectBackend` on purpose.
    fn suspend(&self) -> Result<()> {
        DirectBackend.suspend()
    }

    /// Unlike `DirectBackend::poweroff`, this returns as soon as the
    /// request has been handed off, not once the machine has actually
    /// powered off -- the real transition happens moments later, once
    /// mitos-services finishes stopping its supervised services and
    /// mitos-init acts on the acknowledgement.
    fn poweroff(&self) -> Result<()> {
        signal::kill(Pid::from_raw(1), Signal::SIGTERM)?;
        Ok(())
    }

    /// See `poweroff`'s doc comment -- same asynchronous hand-off, via
    /// SIGINT instead of SIGTERM.
    fn reboot(&self) -> Result<()> {
        signal::kill(Pid::from_raw(1), Signal::SIGINT)?;
        Ok(())
    }
}
