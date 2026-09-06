use calloop::signals::Signal;

/// What a raw signal means to mitos-session, decoupled from which
/// specific POSIX signal caused it so call sites match on intent
/// rather than signal numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalEvent {
    /// SIGTERM or SIGINT: shut down gracefully.
    Terminate,
    /// SIGHUP: re-read `session.toml`.
    ReloadConfig,
    /// SIGCHLD: a child process (compositor, autostart app) exited;
    /// go reap it.
    ReapChildren,
}

/// The signal set mitos-session asks calloop to deliver via a
/// `signalfd`-backed source (so handling happens on the daemon's
/// normal thread, with none of the usual async-signal-safety limits
/// of a real signal handler).
pub const WATCHED: &[Signal] = &[Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP, Signal::SIGCHLD];

/// Map a raw signal to what it means here. `None` for anything not in
/// `WATCHED` (shouldn't happen, but a signal source misconfiguration
/// shouldn't panic the daemon).
pub fn classify(signal: Signal) -> Option<SignalEvent> {
    match signal {
        Signal::SIGTERM | Signal::SIGINT => Some(SignalEvent::Terminate),
        Signal::SIGHUP => Some(SignalEvent::ReloadConfig),
        Signal::SIGCHLD => Some(SignalEvent::ReapChildren),
        _ => None,
    }
}
