# IPC protocol

mitos-session listens on a Unix domain socket (`[ipc].socket_path`,
default `/run/mitos-session/session.sock`, mode `0660`). There is no
separate authentication handshake on the socket -- identity comes
entirely from `SO_PEERCRED` (see `security.md`).

## Framing

Every message, in both directions, is:

```
+----------------+----------------------------+
| length (u32 LE)|  bincode-encoded payload    |
+----------------+----------------------------+
```

Implemented in `ipc::protocol::{read_message, write_message}`. Clients
write a `Request` and read a `Message`; the daemon does the reverse.
Messages larger than 16 MiB are rejected outright (`SessionError::Protocol`)
rather than accepted -- there's no legitimate request or reply anywhere
near that size, so a client claiming one is either broken or hostile.

## Message catalog

### `Request` (client → daemon)

| Variant | Who sends it | Notes |
|---|---|---|
| `CreateSession { user_name, seat_id, session_type }` | a login prompt / greeter | root, or the account itself |
| `TerminateSession { session_id }` | greeter, session owner | logout |
| `RegisterCompositor { session_id }` | mitos-gui | required before this connection receives `Event`s |
| `ListSessions` | anyone | |
| `SessionStatus { session_id }` | session owner, root | |
| `LockSession { session_id }` | session owner, root | |
| `Unlock { session_id, user_name, password }` | mitos-gui (relaying what the user typed) | session owner, root |
| `ReportActivity { seat_id }` | mitos-gui | coalesced input ping, resets idle timers |
| `SwitchSession { seat_id, session_id }` | session owner, root | VT-switch equivalent |
| `Inhibit { what, who, why, mode }` | any app that wants to delay idle/lock/suspend/shutdown | anyone |
| `ReleaseInhibit { inhibit_id }` | whoever holds it | anyone |
| `ListInhibitors` | anyone | |
| `Suspend` / `Reboot` / `PowerOff` | anyone locally authenticated | see `security.md` for why this isn't root-only |
| `RequestElevation { session_id, action }` | mitos-service | root, or `[elevation].service_user` specifically -- see `security.md`'s Elevation section. **Reply is deferred**, see below. |
| `RespondElevation { request_id, response }` | mitos-gui, relaying what the user did with a prompt | root, or the exact compositor connection that prompt was shown to |

`RequestElevation` is the one exception to "any number of `Request`s,
in any order": its `Response` doesn't arrive until the prompt it opens
is resolved by a matching `RespondElevation`, by its session ending, or
by timing out (`[elevation].prompt_timeout_secs`) -- so a caller should
expect this specific call to block for as long as it takes a human to
respond. Nothing about this protocol's `Response` carries a request id
to correlate several in-flight calls on one connection, so a caller
that wants to check more than one action at a time should open a
separate connection per check (the same one-shot pattern `IpcClient`
already uses below), rather than try to multiplex them over a single
long-lived one the way mitos-gui does for `Event`s.

### `Response` (daemon → client, one per `Request`)

`Ok`, `Sessions(Vec<SessionInfo>)`, `Session(SessionInfo)`,
`AuthResult(AuthOutcome)`, `InhibitGranted { inhibit_id }`,
`Inhibitors(Vec<InhibitorInfo>)`, `Error(String)`.

`AuthResult` is shared by three flows, not just `Unlock`: it's also
what `RequestElevation` eventually gets back (once the prompt it opened
resolves) and what `RespondElevation` gets back immediately for the one
attempt it just made. `AuthOutcome::Cancelled` only ever comes from the
elevation flow -- lock/unlock has no "the user declined" concept of its
own, only a password that's right or isn't.

### `Event` (daemon → client, unsolicited)

Only sent to a connection that previously sent `RegisterCompositor`
for the relevant session: `ShowLockScreen`, `HideLockScreen`,
`AuthFeedback`, `Dim`, `Undim`, `PrepareForSleep`, `ResumedFromSleep`,
`SessionActivated`, `ShowElevationPrompt`, `ElevationFeedback`,
`HideElevationPrompt`.

The three elevation events mirror the lock-screen ones' shape:
`ShowElevationPrompt` carries everything needed to render the prompt
(the app/description/risk/duration mitos-service supplied, purely as
display text -- mitos-session doesn't interpret any of it, see
`security.md`); `ElevationFeedback` reports one attempt's outcome
without necessarily closing the prompt (a plain `Failure` leaves it
open for another try, exactly like `AuthFeedback` does for unlock);
`HideElevationPrompt` says the prompt is over, whatever the reason
(answered, cancelled, timed out, or its session ended).

Both `Response` and `Event` travel wrapped in a `Message` enum so a
client only ever needs one `read_message::<_, Message>()` loop.

## Connection lifecycle

1. Client connects; the daemon reads `SO_PEERCRED` immediately and
   registers the connection's outbox with `ConnRegistry` *before*
   reading anything else, so a request that produces an instant reply
   can't race ahead of the connection being known.
2. Client sends any number of `Request`s, in any order -- there's no
   required handshake beyond an optional `RegisterCompositor`, with
   one exception (`RequestElevation`'s deferred reply, above).
3. On disconnect, `ConnRegistry` drops the connection's outbox. If it
   was a session's registered compositor, that session's
   `compositor_conn` is cleared (see `Daemon::handle_ipc` in
   `src/main.rs`); a full implementation would trigger a
   relaunch-with-backoff here (tracked in the README roadmap). Any
   elevation prompt the dropped connection was involved in -- as the
   compositor that would show it, or as the caller waiting on its
   answer -- is abandoned rather than left to time out on its own
   (`ElevationManager::abandon_connection`).

## Client implementations

- `ipc::IpcClient` -- blocking one-shot request/reply, used by
  `mitos-sessionctl`. Also the natural shape for mitos-service to use
  for `RequestElevation`, one connection per check -- see that
  request's note above.
- mitos-gui keeps a long-lived connection open and reads `Message`s in
  a loop instead, since it needs to receive `Event`s at any time, not
  just as a reply to something it asked.
