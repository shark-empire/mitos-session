//! `SystemPowerBackend` that delegates the actual kernel transition to
//! mitos-power's IPC instead of touching `/sys/power/state` or
//! `reboot(2)` here -- see `mod.rs`'s doc comment for why this is the
//! default.
//!
//! mitos-power speaks a different wire format from mitos-session's own
//! (newline-delimited JSON over a Unix socket, not length-prefixed
//! bincode), so this doesn't reuse `crate::ipc`'s client machinery -- it's
//! a small, self-contained blocking client matching the protocol
//! documented in mitos-power's own `docs/ipc.md`. Not independently
//! verified against a running mitos-power (no instance of it was
//! available while writing this); see README.md's status note.

use super::SystemPowerBackend;
use crate::errors::{Result, SessionError};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub struct MitosPowerBackend {
    socket_path: PathBuf,
}

impl MitosPowerBackend {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// One request, one response, matching mitos-power's protocol:
    /// `{"kind":"request","id":...,"method":...,"params":{}}` out,
    /// `{"kind":"response","id":...,"ok":...,...}` in. A `"kind":"event"`
    /// line is possible in principle but this connection never sends
    /// `subscribe`, so none should arrive; skip defensively rather than
    /// mistake one for the reply.
    fn call(&self, method: &str) -> Result<()> {
        let stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| SessionError::Protocol(format!("connecting to mitos-power at {}: {e}", self.socket_path.display())))?;
        stream.set_read_timeout(Some(Duration::from_secs(10))).map_err(|e| SessionError::Protocol(format!("setting read timeout: {e}")))?;
        let mut writer = stream.try_clone().map_err(|e| SessionError::Protocol(format!("cloning socket to mitos-power: {e}")))?;
        writer.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| SessionError::Protocol(format!("setting write timeout: {e}")))?;
        let mut reader = BufReader::new(stream);

        let request = json!({ "kind": "request", "id": "mitos-session", "method": method, "params": {} });
        let mut line = serde_json::to_string(&request).map_err(|e| SessionError::Protocol(format!("encoding request to mitos-power: {e}")))?;
        line.push('\n');
        writer.write_all(line.as_bytes()).map_err(|e| SessionError::Protocol(format!("writing to mitos-power: {e}")))?;

        loop {
            let mut raw = String::new();
            let n = reader.read_line(&mut raw).map_err(|e| SessionError::Protocol(format!("reading from mitos-power: {e}")))?;
            if n == 0 {
                return Err(SessionError::Protocol("mitos-power closed the connection without replying".into()));
            }
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed).map_err(|e| SessionError::Protocol(format!("parsing mitos-power reply: {e}")))?;
            if value.get("kind").and_then(Value::as_str) != Some("response") {
                continue;
            }
            return if value.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                Ok(())
            } else {
                let message = value.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("unknown error");
                Err(SessionError::Protocol(format!("mitos-power rejected {method}: {message}")))
            };
        }
    }
}

impl SystemPowerBackend for MitosPowerBackend {
    fn suspend(&self) -> Result<()> {
        // mitos-power's Suspend does not return until the machine has
        // actually resumed -- this blocks for the whole sleep duration,
        // same as DirectBackend's raw /sys/power/state write already did.
        self.call("Suspend")
    }

    fn poweroff(&self) -> Result<()> {
        // On success the machine powers off mid-call and this never
        // returns -- same caveat as DirectBackend::poweroff.
        self.call("Poweroff")
    }

    fn reboot(&self) -> Result<()> {
        self.call("Reboot")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A stand-in mitos-power: reads one NDJSON request line and replies
    /// with the given outcome. Binding happens on the test thread before
    /// this returns, so the caller can connect immediately afterward with
    /// no sleep-based race -- the OS queues the incoming connection in the
    /// listen backlog until the spawned thread calls `accept()`.
    fn fake_mitos_power(ok: bool, message: &'static str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mitos-session-power-backend-test-{}-{}", std::process::id(), ok));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("power.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(line.trim()).unwrap();
            let reply = if ok {
                json!({ "kind": "response", "id": request["id"], "ok": true, "result": {} })
            } else {
                json!({ "kind": "response", "id": request["id"], "ok": false, "error": { "code": "HARDWARE_ERROR", "message": message } })
            };
            let mut out = serde_json::to_string(&reply).unwrap();
            out.push('\n');
            stream.write_all(out.as_bytes()).unwrap();
        });

        socket
    }

    #[test]
    fn successful_reply_is_ok() {
        let socket = fake_mitos_power(true, "");
        assert!(MitosPowerBackend::new(socket).reboot().is_ok());
    }

    #[test]
    fn error_reply_surfaces_the_message() {
        let socket = fake_mitos_power(false, "no 'disk' support reported by the kernel");
        let err = MitosPowerBackend::new(socket).suspend().unwrap_err().to_string();
        assert!(err.contains("no 'disk' support"), "got: {err}");
    }

    #[test]
    fn unreachable_socket_is_an_error_not_a_panic() {
        let missing = std::env::temp_dir().join(format!("mitos-session-no-power-{}", std::process::id())).join("absent.sock");
        assert!(MitosPowerBackend::new(missing).poweroff().is_err());
    }
}
