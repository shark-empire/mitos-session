# Architecture

## Threading model

Every piece of mutable daemon state -- every session, seat, lock, and
inhibitor -- lives in one `Daemon` struct (`src/main.rs`) that is only
ever touched from calloop callbacks running on a single thread. There
is no `Arc<Mutex<...>>` anywhere in the domain model.

```
                    ┌─────────────────────────────────────────┐
                    │           calloop event loop             │
                    │            (single thread)                │
                    │                                            │
   IPC connection    │  ┌──────────────┐   ┌─────────────────┐  │
   reader threads ───┼─▶│ cmd_channel  │──▶│  Daemon (state)  │  │
   (one per client)  │  └──────────────┘   └────────┬─────────┘  │
                    │                                │            │
                    │  ┌──────────────┐              │            │
                    │  │ idle timer   │─────────────▶│            │
                    │  │  (1 Hz)      │              │            │
                    │  └──────────────┘              │            │
                    │                                │            │
                    │  ┌──────────────┐              │            │
                    │  │ signalfd     │─────────────▶│            │
                    │  │ (SIGTERM...) │              │            │
                    │  └──────────────┘              │            │
                    └─────────────────────────────────────────┘
                                          │
                                          ▼
                         ConnRegistry.send_response/send_event
                                          │
                                          ▼
                          per-connection writer thread ──▶ socket
```

IPC connection threads (`ipc::server`) only ever move bytes: a reader
thread blocks on `read_message`, wraps whatever it gets in a
`ManagerCommand`, and hands it to the calloop thread over a channel. A
writer thread does the mirror image for outgoing `Response`/`Event`
messages. Neither thread ever reads or mutates a `Session`, `Seat`, or
lock directly.

This buys two things: no lock contention or poisoning to reason about
in the domain model, and a natural place (`policy::authorize`) to make
every authorization decision before any state changes -- there's
exactly one call site where an IPC request turns into a mutation.

## Data flow: locking a session

1. mitos-gui detects idle input (or the user hits a lock shortcut) and
   sends `Request::LockSession` (or the daemon's own idle timer fires
   past `lock_after_secs` -- see `idle::IdleDetector`).
2. `policy::authorize` checks the caller owns the session (or is root).
3. `lock::LockManager::lock` checks for a blocking inhibitor, then
   flips the session's `LockState`.
4. `session::SessionContext::transition` moves the coarse
   `SessionState` to `Locked`.
5. If a compositor is registered for that session, `ConnRegistry`
   pushes `Event::ShowLockScreen` to it -- mitos-gui draws the lock
   screen and forwards whatever the user types back as
   `Request::Unlock`.
6. `authentication::check` runs it through PAM; `LockManager` updates
   attempt counts / lockout state and the outcome is relayed back as
   `Event::AuthFeedback` (and the `Response::AuthResult` for the
   original request).

## Where mitos-gui and mitos-services fit in

- **mitos-gui** never decides *whether* to lock, dim, or suspend. It
  renders what mitos-session tells it to and reports input activity
  and unlock attempts back. See `../mitos-gui`.
- **mitos-services** isn't integrated yet. `power::SystemPowerBackend`
  is the seam where a real install should route suspend/reboot/poweroff
  through it (to stop services in dependency order) instead of calling
  `reboot(2)` directly the way `power::DirectBackend` does today.

## Known gaps

- `seat::device` enumerates real hardware via `udev` at startup, but
  it's a snapshot, not a live view -- hotplugging a device in doesn't
  update it until the daemon restarts. Real hotplug tracking needs a
  `udev::MonitorSocket`'s raw fd wired into the calloop loop as its
  own event source, which is a bigger change than static enumeration
  and is still a follow-up.
- `user::groups` reads `/etc/group` directly rather than going through
  NSS, so directory-backed (LDAP/SSSD) accounts won't see their full
  supplementary group list.
- There's no polkit-equivalent: the authorization rules in
  `policy::security_policy` are a fixed, hard-coded policy rather than
  something an admin can reconfigure per-action.
- Compositor crash-restart (`Daemon::handle_compositor_exit`) tracks
  restart count per session but not a time window -- a compositor that
  crashes, runs fine for an hour, then crashes again picks up the
  counter where it left off rather than it decaying. Fine for now
  since `max_compositor_restarts` is small, but worth revisiting if it
  turns out to matter in practice.
