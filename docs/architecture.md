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

## Data flow: an elevation prompt

Three parties instead of lock/unlock's two -- see `docs/security.md`'s
Elevation section for the trust reasoning behind each authorization
check below.

1. mitos-service decides (via its own rulebook, outside this repo)
   that an app's attempted action needs the logged-in user's password,
   and sends `Request::RequestElevation { session_id, action }`.
2. `policy::authorize` checks the caller is `[elevation].service_user`
   or root -- nobody else may open a prompt.
3. `Daemon::start_elevation` fails fast (still a normal, immediate
   `Response`) if elevation is disabled, the session doesn't exist or
   has no password configured, or it's already locked out or already
   at `[elevation].max_pending_per_session`. Otherwise
   `ElevationManager::begin` records a pending request and starts its
   timeout, and mitos-service's call stops here -- its `Response`
   won't arrive until this prompt resolves one way or another.
4. `ConnRegistry` pushes `Event::ShowElevationPrompt` to the session's
   registered compositor. mitos-gui draws it (only the compositor
   can -- see `docs/security.md`) and forwards what the user did back
   as `Request::RespondElevation`.
5. `policy::authorize` checks the answer arrived on the exact
   connection the prompt was shown to, or root.
6. `Daemon::respond_elevation` resolves it: a cancel skips PAM
   entirely; a password goes through `authentication::check` exactly
   like unlock does, against elevation's own `AuthPolicy`
   (`policy::elevation_auth_policy`) and its own, separate attempt
   counter. The outcome goes back to mitos-gui as
   `Event::ElevationFeedback` immediately; a *terminal* outcome
   (anything but a plain retry-able failure) additionally closes the
   prompt out with `Event::HideElevationPrompt` and finally delivers
   the long-deferred `Response::AuthResult` to mitos-service from
   step 3.
7. If nobody ever answers -- the compositor crashed, the user walked
   away, the session logged out -- `Daemon::on_idle_tick`'s timeout
   sweep or the relevant disconnect/termination handling abandons the
   prompt the same way step 6 would on an `Error` outcome, so
   mitos-service's connection is never left blocked forever.

## Where mitos-gui, mitos-service, and mitos-services fit in

- **mitos-gui** never decides *whether* to lock, dim, suspend, or
  approve a privileged action. It renders what mitos-session tells it
  to and reports input activity, unlock attempts, and elevation
  answers back. See `../mitos-gui`.
- **mitos-service** (singular) is the permission-policy daemon that
  owns MITOS's rulebook of what apps may do -- classifying an action's
  risk and deciding it needs a password is entirely its job, not
  this repo's. What lives here is only mitos-session's side of that
  conversation (`elevation`, `Request::RequestElevation`/
  `RespondElevation`): proving the credential is right, the same way
  `authentication` already does for unlock. mitos-service itself is a
  separate component this repo has no visibility into.
- **mitos-services** (plural) -- easy to misread as the same thing as
  the above, and deliberately not -- is the process supervisor and
  isn't integrated yet. `power::SystemPowerBackend` is the seam where
  a real install should route suspend/reboot/poweroff through it (to
  stop services in dependency order) instead of calling `reboot(2)`
  directly the way `power::DirectBackend` does today.

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
- There's no `Request` for mitos-service to withdraw an elevation
  request it already sent -- if the app that wanted the privileged
  action closes, or mitos-service itself decides it no longer needs an
  answer, before the user responds, the prompt still sits on screen
  until it's answered or `[elevation].prompt_timeout_secs` elapses.
  Not a security problem (nothing is granted just because a stale
  prompt exists), just a UX one worth a follow-up
  `Request::CancelElevation` if it turns out to matter in practice.
