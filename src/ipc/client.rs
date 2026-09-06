use super::messages::{Message, Request, Response};
use super::protocol::{read_message, write_message};
use crate::errors::{Result, SessionError};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Blocking request/reply client for mitos-session's IPC socket. Used
/// by `mitos-sessionctl`. mitos-gui speaks the same wire format but
/// keeps its connection open long-term to receive `Event`s, so it uses
/// `protocol::{read_message, write_message}` directly instead of this
/// one-shot wrapper.
pub struct IpcClient {
    stream: UnixStream,
}

impl IpcClient {
    pub fn connect(socket_path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).map_err(|e| {
            SessionError::Io(std::io::Error::new(
                e.kind(),
                format!("could not connect to {}: {e}", socket_path.display()),
            ))
        })?;
        Ok(Self { stream })
    }

    /// Send `request` and wait for the matching `Response`. Any
    /// `Event`s that arrive first -- there normally aren't any on a
    /// connection that never registered as a compositor -- are logged
    /// and skipped rather than treated as an error.
    pub fn call(&mut self, request: Request) -> Result<Response> {
        write_message(&mut self.stream, &request)?;
        loop {
            match read_message::<_, Message>(&mut self.stream)? {
                Message::Response(resp) => return Ok(resp),
                Message::Event(event) => {
                    tracing::debug!(?event, "ignoring unsolicited event on a request/response connection");
                }
            }
        }
    }
}
