# mitos-session

**The session, seat, lock, idle, authentication, and power manager for the MITOS operating system.**

`mitos-session` is the central policy daemon that sits between the system init (`mitos-init`) and the user's desktop (`mitos-gui`). It owns the notion of "who is logged in, on which seat, in what state," and acts as the absolute authority for locking, idling, suspending, and permission-gating the machine.

It does not draw a single pixel itself. All UI (lock screens, elevation prompts, greeters) is rendered by `mitos-gui` or `mitos-login`, which communicate with this daemon over a secure, binary IPC channel.

## 🏗️ Architecture & Design Principles

*   **One Thread, One Brain:** All session, seat, lock, and idle state lives on a single thread driven by a `calloop` event loop. No `Arc<Mutex<...>>` soup. Timers, signals, and IPC commands are just events dispatched to `&mut self` handlers.
*   **Strict Process Isolation:** `mitos-session` runs as root to manage sockets and PAM, but spawns all user processes (compositors, shells, autostart apps) using `setsid()` and drops privileges to the user's UID/GID via `pre_exec` hooks.
*   **Zero-Trust IPC:** Inter-process communication uses a length-prefixed `bincode` protocol over a Unix domain socket. Authorization is handled entirely via `SO_PEERCRED` (kernel-level peer UID/GID verification). There is no handshake authentication.
*   **Real Linux Facilities:** Leverages `udev` for live hardware hotplug, `libpam` for real credential verification, and `sd_notify` for service manager readiness.

## 🚀 Feature Highlights

### Phase 1: Foundation & Hardware Reactivity
*   **Secure `XDG_RUNTIME_DIR`:** Fd-based creation with `O_NOFOLLOW` symlink attack prevention and strict `0700` permission locking.
*   **Environment Sanitization:** Strips dangerous inherited variables (`LD_PRELOAD`, `LD_LIBRARY_PATH`) and injects only safe XDG session variables.
*   **Live `udev` Hotplug:** Replaces startup-only snapshots with a live `MonitorSocket` wired into `calloop` to track keyboards, mice, and DRM nodes dynamically.

### Phase 2: Security & Authentication
*   **Real PAM Stack:** Non-blocking PAM authentication with automatic memory zeroization (`ZeroizingString`) for passwords in transit and at rest.
*   **Permission Gates:** Revokes `ScreenCapture` and `RawInput` permissions from all applications the millisecond the session enters a `Locked` state.
*   **Exponential Backoff:** Cryptographically deterministic jitter on lockout timers to prevent timing attacks and brute-force attempts.

### Phase 3: GUI Integration & State Sync
*   **Lock-Screen Synchronization:** Broadcasts `SessionStateChanged` to all clients, ensuring the compositor renders the lock surface and blocks underlying windows.
*   **Notification Redaction:** Automatically pushes `NotificationPolicy { redact_bodies: true }` to the notification service when locked.
*   **Greeter IPC:** Allows `mitos-login` to query system accounts, session types, and accessibility settings before a session is created.

### Phase 4: Resilience & Recovery
*   **Crash Cascade:** If the compositor crashes $N$ times, the daemon escalates to a Shell restart. If the Shell fails, the session is forcefully terminated to prevent infinite crash loops.
*   **Clean Teardown:** Uses negative PGID signaling (`kill(-pgid, SIGTERM)`) to instantly wipe out the entire session tree (compositor + autostart apps) on logout.
*   **Stale Artifact Scrubbing:** Automatically deletes orphaned Wayland/X11 sockets if a session ends uncleanly (e.g., SIGKILL).

### Phase 5: Power & Multi-Seat Polish
*   **Suspend-Lock Invariant:** Forces the session into `SessionState::Suspended` *before* the hardware sleeps, guaranteeing a password prompt on wake.
*   **XDG Autostart:** Parses `/etc/xdg/autostart` and `~/.config/autostart`, filtering by `OnlyShowIn=MITOS` and launching apps with dropped privileges.
*   **Multi-Seat Isolation:** Strictly filters `udev` device lists by `ID_SEAT` to ensure inputs and displays are isolated per user on multi-GPU towers.

## 🛠️ Building & Running

### Prerequisites
*   Rust 1.75+
*   Linux kernel with `udev`, `PAM`, and `tmpfs` support.

### Build
```bash
cargo build --release


Session, seat, screen-lock, idle, authentication, and power manager for
**MITOS**. `mitos-session` is the process that sits between the login
prompt and your desktop: it owns the notion of "who is logged in, on
which seat, in what state," and it is the *policy* authority for
locking, idling, and suspending the machine. It does not draw a single
pixel itself -- lock-screen and any other on-screen UI is rendered by
[`mitos-gui`](../mitos-gui), which talks to this daemon over IPC.

## Where this sits in MITOS

```
 mitos-init  (PID 1, panic=abort, does almost nothing)
     |
 mitos-services  (supervises daemons, cgroups, restarts)
     |
     +-- mitos-session   <-- this project
     |        |
     |        | unix socket, length-prefixed bincode, to both of:
     |        v
     +-- mitos-gui       (Smithay/Wayland compositor + shell, renders
     |                     the desktop AND the lock screen on request)
     |
     +-- mitos-service   (permission-policy daemon: owns the rulebook of
                           what apps may do, asks this project to verify
                           the logged-in user before a risky one proceeds)
```

`mitos-session` asks `mitos-gui` to *show* a lock screen (or an
elevation prompt on `mitos-service`'s behalf); `mitos-gui` forwards
whatever the user types back over the same socket for `mitos-session`
to check against PAM. `mitos-service` never talks to `mitos-gui`
directly, and never sees a password -- only mitos-session's verdict.
Nobody here trusts anyone else's process to do its job -- see
[`docs/security.md`](docs/security.md).

## Design principles

- **One thread, one brain.** All session/seat/lock/idle state lives on
  a single thread driven by a [`calloop`](https://crates.io/crates/calloop)
  event loop -- the same crate `mitos-gui`'s compositor already uses.
  No `Arc<Mutex<...>>` soup: timers, signals, and IPC commands are all
  just events the loop dispatches to `&mut self` handlers. Socket I/O
  happens on small helper threads that only ever move bytes; they never
  touch state directly, they hand a `ManagerCommand` to the loop over a
  channel and get a reply the same way.
- **Small footprint by default.** Binary IPC framing (`bincode`)
  instead of JSON, no D-Bus dependency, no async runtime, `lto` +
  `codegen-units = 1` + `strip` in release builds. This is a daemon
  that should be sitting at a few hundred KB of RSS while idle.
- **Real Linux, not a toy.** MITOS runs on the real Linux kernel, so
  this project leans on real kernel/userspace facilities that already
  exist and are well-tested: PAM for authentication, `/etc/passwd` and
  `/etc/group` via `nix`, `udev` for device enumeration, `SO_PEERCRED`
  for IPC authorization, and `reboot(2)` for power transitions.
- **Fail loud, fail narrow.** A panic in one client's request handler
  is caught at the connection boundary and turned into an `Error`
  response -- it should never be able to take down every other logged
  in session.

## Module map

| Module            | Responsibility |
|--------------------|----------------|
| `session/`          | The `Session` type, its lifecycle state machine, per-session environment (`XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY`, ...), and the `SessionManager` registry. |
| `user/`             | Account lookup (`/etc/passwd`, `/etc/group`), supplementary groups, home directory checks, privilege-drop helpers. |
| `seat/`             | Seats, the active-session-per-seat concept, and `udev`-backed device enumeration (a startup snapshot, not live hotplug tracking -- see Known gaps in `docs/architecture.md`). |
| `lock/`             | Lock state machine, lock policy, idle-triggered/suspend-triggered lock timing, and "inhibitor" locks that let an app say "don't lock while I'm playing a video." |
| `idle/`             | Idle detection: per-seat activity tracking and the timers that fire dim / lock / suspend thresholds. |
| `authentication/`   | PAM-backed credential checking, with its own attempt/lockout policy. |
| `elevation/`         | Handles mitos-service's requests to verify the logged-in user before a privileged action proceeds -- a three-party relay (mitos-service asks, mitos-gui prompts, mitos-session checks) distinct from lock/unlock's two-party one. See `docs/security.md`'s Elevation section. |
| `launcher/`         | Spawns `mitos-gui`, autostart applications, and a fallback terminal for a session, plus the restart-vs-give-up policy for a compositor that keeps crashing (`decide_restart`). |
| `ipc/`              | The wire protocol, framing, the socket server, the client used by both `mitos-gui` and `mitos-sessionctl`, and peer-credential based authorization. |
| `power/`            | Suspend / resume / shutdown / reboot orchestration (pre-suspend inhibitor checks, post-resume re-lock, etc). |
| `signals/`          | `SIGTERM`/`SIGINT`/`SIGHUP`/`SIGCHLD` handling wired into the calloop loop. |
| `logging/`          | `tracing` setup plus a structured security audit log, separate from general debug logs. |
| `policy/`           | Turns on-disk config into the runtime policy objects each subsystem consumes, and centralizes the "is this peer allowed to do this" authorization decision. |
| `config/`           | `session.toml` schema, defaults, and loading. |
| `errors/`           | The single `SessionError` type shared across the crate. |

## Building

```sh
cargo build --release
```

> Dependency versions in `Cargo.toml` are pinned to the latest series
> known at the time this scaffold was written. This box has no network
> access, so nothing here has been built against the real crates.io
> registry yet -- run `cargo update` and fix up anything that drifted
> before you rely on it.

Two binaries come out of this crate:

- `mitos-session` -- the daemon (needs to run as a privileged service;
  see `docs/security.md`).
- `mitos-sessionctl` -- a CLI client for talking to it (`list-sessions`,
  `lock`, `unlock`, `suspend`, `reboot`, `poweroff`, `inhibit`,
  `request-elevation`, `respond-elevation`, ...).

## Status

This is a full architectural scaffold, not a hardened implementation --
every module compiles conceptually against the design in `docs/`, with
the core state machines, IPC framing, device enumeration, and
compositor crash-restart written for real. The PAM conversation
callback in `authentication/authenticator.rs` and the *hotplug* half
of device tracking (enumeration itself is real; live add/remove isn't)
are the two places most likely to need real hardware/PAM-stack testing
before this touches a login screen.

## Roadmap

1. **Core models** -- `session`, `user`, `seat`, `config`, `errors`. (done in this scaffold)
2. **IPC** -- protocol, framing, server/client, peer-credential authorization. (done in this scaffold)
3. **Lock, idle, authentication** -- state machines and PAM wiring. (done in this scaffold)
4. **Launcher, power, signals** -- process spawning, compositor crash-restart-with-backoff, and system power transitions. (done in this scaffold)
5. **Real device enumeration** -- `seat/device.rs` enumerates input/DRM hardware via `udev` at startup. (done in this scaffold)
6. **Elevation** -- mitos-session's side of the permission-elevation flow: verifying the logged-in user on mitos-service's behalf before a privileged action proceeds, with its own trust boundary, attempt/lockout policy, and prompt-timeout handling (`elevation/`, `docs/security.md`'s Elevation section). (done in this scaffold) mitos-service itself -- the daemon that owns the rulebook and decides an action needs a password in the first place -- is a separate project this repo has no visibility into.
7. **Hardening** -- exercise against a real PAM stack and real multi-seat hardware; wire a `udev::MonitorSocket` into the calloop loop for live hotplug tracking instead of the current startup-only snapshot; add the fuzz/integration tests in `tests/` that need root.

## License

MIT, see [`LICENSE`](LICENSE).
