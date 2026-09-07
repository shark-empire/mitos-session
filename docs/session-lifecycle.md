# Session lifecycle

## States

`session::SessionState` (see `src/session/state.rs` for the exact
transition table enforced in code):

```
 Starting ──────────────▶ Active ─────┬────▶ Idle ─────┬────▶ Locked
     │                      │  ▲      │       │  ▲      │       │  ▲
     │                      │  └──────┘       │  └──────┘       │  │
     │                      │                 │                 │  │
     │                      │◀────────────────┴─────────────────┘  │
     │                      │                                      │
     │                      ▼                                      │
     └─────────────────▶ Closing ◀─────────────────────────────────┘
                            │
                            ▼
                          Closed
```

- **Starting** -- `Request::CreateSession` has just been accepted
  (user's home directory confirmed usable, `XDG_RUNTIME_DIR` exists,
  and for non-`tty` sessions `mitos-gui` has been spawned but hasn't
  registered itself yet).
- **Active** -- `Request::RegisterCompositor` arrived and the session
  wasn't locked: it has a live display and owns its seat's active
  slot.
- **Idle** -- `idle::IdleDetector` crossed the `dim_after_secs`
  threshold for this session's seat. Purely informational; nothing
  about authentication changes here.
- **Locked** -- `Request::LockSession`, or the idle timer crossing
  `lock_after_secs` (if `lock_on_idle` is set). Only a successful
  `Request::Unlock` moves back to `Active`.
- **Closing** -- `Request::TerminateSession`, or the daemon shutting
  down (`SIGTERM`/`SIGINT`) and terminating every session itself.
  The compositor process is killed and reaped here.
- **Closed** -- terminal; the session is removed from
  `SessionManager`'s registry immediately after.

## When a compositor crashes

The diagram above is the happy path. `Active`, `Idle`, and `Locked`
can all also fall back to `Starting` -- not shown, to keep the diagram
readable -- when `SIGCHLD` reports that a session's compositor process
exited on its own rather than through `TerminateSession`
(`Daemon::handle_compositor_exit` in `src/main.rs`):

1. The dead process is cleared out and its (now-stale) IPC connection
   is unregistered.
2. The session drops to `Starting` -- it isn't gone, it just has no
   display for a moment.
3. `launcher::decide_restart` checks `compositor_restarts` against
   `[session].max_compositor_restarts`: under the limit, mitos-gui is
   relaunched (`Restart`); at the limit, a plain shell is launched
   instead (`FallbackToTerminal`) so there's still something to work
   from and debug.
4. Once a (relaunched or fallback) process registers itself via
   `RegisterCompositor`, the session becomes `Active` again -- *unless*
   `LockManager` still considers it locked, in which case it goes
   straight to `Locked` and the fresh compositor is told to show the
   lock screen again. A crash never unlocks a session.

The restart counter is per-session and doesn't currently decay over
time (see `docs/architecture.md`'s Known gaps) -- a compositor that
crashes once, runs fine for hours, and then crashes again picks up the
count where it left off.

## Idle -> lock -> suspend chain

`idle::IdlePolicy` (built from `[idle]` config) defines three
thresholds against the same "seconds since last input" measurement:
`dim_after_secs < lock_after_secs < suspend_after_secs`. Setting any of
them to `0` disables that stage entirely rather than firing
immediately -- see `IdlePolicy::stage_for`.

The idle detector itself has no idea what "lock" or "suspend" mean; it
just reports stage transitions once a second (`Daemon::on_idle_tick` in
`src/main.rs`), and the daemon's own code decides what to do about
each one: dim sends `Event::Dim` to the active session's compositor,
`LockRequested` calls `lock::LockManager::lock` (only if
`[lock].lock_on_idle` is set), and `SuspendRequested` calls
`power::suspend`.

## Suspend and resume

`power::suspend` locks every session first (if `[lock].lock_on_suspend`
is set) and sends `Event::PrepareForSleep` to every registered
compositor *before* handing off to the kernel -- there's no need to
re-lock on wake, since nothing can happen to an already-locked session
while the machine is suspended. `power::on_resume` just tells
compositors the display is back (`Event::ResumedFromSleep`) so they can
redraw and re-check the lock screen's clock.

## Multiple sessions per seat

A seat has one *active* session and a FIFO queue of everyone else
logged in but switched away from (`seat::Seat`). Logging out of the
active session promotes the next queued one automatically
(`SeatManager::detach_session`); `Request::SwitchSession` does it
manually and sends the newly-active session's compositor
`Event::SessionActivated`.
