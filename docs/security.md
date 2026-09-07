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
