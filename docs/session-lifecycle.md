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

- **Starting** -- `Request::CreateSession` has been accepted, the
  user's home directory is confirmed usable, `XDG_RUNTIME_DIR` exists,
  and (for non-`tty` sessions) `mitos-gui` has been spawned but hasn't
  registered itself yet.
- **Active** -- `Request::RegisterCompositor` arrived: the session has
  a live display and owns its seat's active slot.
- **Idle** -- `idle::IdleDetector` crossed the `dim_after_secs`
  threshold for this session's seat. Purely informational; nothing
  about authentication changes here.
- **Locked** -- either `Request::LockSession` or the idle timer
  crossing `lock_after_secs` (if `lock_on_idle` is set). Only a
  successful `Request::Unlock` moves back to `Active`.
- **Closing** -- `Request::TerminateSession`, or the daemon shutting
  down (`SIGTERM`/`SIGINT`) and terminating every session itself.
  The compositor process is killed and reaped here.
- **Closed** -- terminal; the session is removed from
  `SessionManager`'s registry immediately after.

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
