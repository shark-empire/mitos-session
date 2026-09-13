# Security model

## Trust boundaries

- **mitos-session runs privileged** (it needs to `setuid`/`setgid`
  before launching a session's processes, and to call `reboot(2)`).
  Every other process on the box -- mitos-gui, mitos-sessionctl, any
  autostart application -- is untrusted input as far as this daemon is
  concerned.
- **Identity comes from the kernel, not the client.** `SO_PEERCRED`
  (`ipc::permissions::peer_credentials`) is the *only* identity
  mitos-session trusts an IPC connection with. Nothing a client says
  about itself in a `Request` is treated as authoritative for who it
  is.
- **PAM is the actual credential check.** mitos-session never sees or
  stores a password hash; `authentication::PamAuthenticator` hands the
  raw password straight to `/etc/pam.d/<pam_service>` and trusts its
  verdict. This is deliberate -- reimplementing password verification
  here would be a strictly worse version of what the system's PAM
  stack already does correctly.

## Authorization

Centralized in `policy::security_policy::authorize`, called once per
`Request` before any state changes (`Daemon::handle_command` in
`src/main.rs`). The policy:

- **root can do anything.**
- **A session's owner (matching uid) can act on their own session** --
  lock it, unlock it, register as its compositor, switch to it,
  terminate it.
- **Starting a session for someone else requires root** (a login
  greeter normally runs as root); starting your own is always fine.
- **System-wide operations -- listing sessions/inhibitors, reporting
  activity, taking an inhibitor, suspend/reboot/poweroff -- are open to
  any locally-authenticated user.** This matches logind's default,
  polkit-less behavior. It is *not* the same as "safe for a
  multi-user, mutually-distrusting machine" -- see Known limitations.

## Elevation

The `elevation` module handles a different kind of request: mitos-service
(the permission-policy daemon that owns MITOS's rulebook of what apps
may do -- not to be confused with mitos-services, plural, the process
supervisor mentioned above) asks mitos-session to verify the logged-in
user before a privileged, app-triggered action proceeds. This is
orthogonal to the screen-lock flow above -- a session can be fully
unlocked and still get an elevation prompt -- and it's a three-party
relay rather than the two-party conversations everything else in this
document describes: mitos-service asks, mitos-gui prompts and answers,
mitos-session checks and replies to both.

Two things make this trust boundary meaningfully stricter than the
"session owner" rule above, and both are deliberate:

- **Only the configured `[elevation].service_user` (or root) may open
  a prompt at all** -- never a session's own owner acting on their own
  behalf, and never "any locally-authenticated user" the way the
  system-wide operations above are. If an ordinary app could trigger a
  prompt directly, it could show a real, trustworthy-looking system
  password box captioned with whatever text it liked ("Chrome wants
  root access") regardless of what it was actually about to do -- a
  textbook phishing setup. Routing every prompt through mitos-service
  means the *reason* shown on screen was chosen by the component that
  actually classified the risk, not by the thing asking for
  credentials to be checked. If the configured account doesn't resolve
  to a uid on this system, elevation requests are accepted from
  nobody but root until it's fixed (`Daemon::resolve_elevation_requester`
  in `src/main.rs`) -- failing open here would turn a typo'd config
  value into a door any process could walk through.
- **Only the exact compositor connection a specific prompt was shown
  to (or root) may answer it** -- not merely "some process running as
  the session owner". Only the compositor can draw an unfakeable
  password prompt in the first place (see `docs/architecture.md`); if
  any same-uid process could also submit an *answer* to a prompt it
  didn't draw, a malicious background app could run its own look-alike
  dialog, relay whatever the user types into it straight through as
  the "real" answer, and launder a phished password through the one
  channel that's supposed to be unspoofable. An unknown request id and
  a wrong-connection attempt are rejected with the identical error on
  purpose (`errors::SessionError::UnknownElevationRequest`) --
  distinguishing them would let a caller probe for which request ids
  currently exist.

A pending request that never gets an answer doesn't linger: it's
abandoned (and mitos-service's still-blocked call replied to with an
error) if its `[elevation].prompt_timeout_secs` elapses, if its session
is terminated, or if the compositor or the original caller disconnects
-- see `ElevationManager::{expire_pending,abandon_session,abandon_connection}`
and `Daemon::notify_elevation_abandoned`. Without this, a crashed
compositor or a session that logged out mid-prompt would leave
mitos-service's connection blocked indefinitely.

Every request, response, lockout, and abandonment is written to the
audit log (`logging::audit_log`), including the app name and action
label mitos-service supplied -- readable later from
`mitos-settings` per the design doc's promise that every grant
decision is traceable to who asked for what and when.

## Privilege dropping

`user::permissions::drop_privileges` and `launcher::Application::spawn`
resolve supplementary groups and build the target uid/gid **in the
parent**, then only make the raw `setgroups`/`setgid`/`setuid` syscalls
inside `pre_exec`, after `fork()` but before `exec()`. No file I/O or
allocation happens in that window -- the usual fork-in-a-threaded-process
hazards around calling non-async-signal-safe code between `fork` and
`exec` don't apply here, because there isn't any such code in that
window to begin with.

## Socket permissions

The IPC socket is created mode `0660` (configurable via
`[ipc].socket_mode`) so only root and members of whatever group owns
it can connect at all -- everyone else is refused at the filesystem
level, before `SO_PEERCRED` even enters into it.

## Lockout policy

`[lock].max_auth_attempts` and `[lock].lockout_secs` bound brute-force
guessing of a session's password over this socket specifically. This
is defense in depth on top of whatever `/etc/pam.d/<service>` already
enforces (e.g. `pam_faillock`) -- it does not replace it.

Elevation attempts are tracked and locked out completely separately,
under `[elevation].max_attempts`/`lockout_secs`, against their own PAM
service (`[elevation].pam_service`, distinct from `[lock]`'s). A wrong
lock-screen guess doesn't burn an elevation attempt and vice versa,
even though both ultimately check the same account's password -- they
guard against different-shaped attacks (unlock: one attacker sitting
at a locked screen trying to guess their way in; elevation: one
attacker hoping a flood of prompts eventually gets fat-fingered
"yes") worth being able to tune independently, and conflating their
counters would mean exhausting one silently ate into the other's
budget.

## Known limitations

- **No polkit-equivalent.** The authorization rules above are fixed in
  code, not admin-configurable per-action. A machine that wants "only
  the physically-present user may suspend it" needs that added as a
  real policy layer, not a config toggle, today.
- **`seat::device` enumerates real hardware, but only once at startup.**
  There's no live hotplug tracking yet (see `docs/architecture.md`'s
  Known gaps) and no real device arbitration between multiple
  simultaneous seats -- this scaffold assumes single-seat until that
  lands.
- **`user::groups` reads `/etc/group` directly**, not through NSS, so
  directory-backed accounts (LDAP/SSSD) may get an incomplete
  supplementary group list when their session's processes are
  launched.
