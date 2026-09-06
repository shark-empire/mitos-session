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

### `Response` (daemon → client, one per `Request`)

`Ok`, `Sessions(Vec<SessionInfo>)`, `Session(SessionInfo)`,
`AuthResult(AuthOutcome)`, `InhibitGranted { inhibit_id }`,
`Inhibitors(Vec<InhibitorInfo>)`, `Error(String)`.

### `Event` (daemon → client, unsolicited)

Only sent to a connection that previously sent `RegisterCompositor`
for the relevant session: `ShowLockScreen`, `HideLockScreen`,
`AuthFeedback`, `Dim`, `Undim`, `PrepareForSleep`, `ResumedFromSleep`,
`SessionActivated`.

Both `Response` and `Event` travel wrapped in a `Message` enum so a
client only ever needs one `read_message::<_, Message>()` loop.

## Connection lifecycle

1. Client connects; the daemon reads `SO_PEERCRED` immediately and
   registers the connection's outbox with `ConnRegistry` *before*
   reading anything else, so a request that produces an instant reply
   can't race ahead of the connection being known.
2. Client sends any number of `Request`s, in any order -- there's no
   required handshake beyond an optional `RegisterCompositor`.
3. On disconnect, `ConnRegistry` drops the connection's outbox. If it
   was a session's registered compositor, that session's
   `compositor_conn` is cleared (see `Daemon::handle_ipc` in
   `src/main.rs`); a full implementation would trigger a
   relaunch-with-backoff here (tracked in the README roadmap).

## Client implementations

- `ipc::IpcClient` -- blocking one-shot request/reply, used by
  `mitos-sessionctl`.
- mitos-gui keeps a long-lived connection open and reads `Message`s in
  a loop instead, since it needs to receive `Event`s at any time, not
  just as a reply to something it asked.
