use crate::errors::{Result, SessionError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};

/// Identifies one live IPC connection for as long as it's open. Not
/// persisted anywhere -- purely an in-memory handle for routing
/// replies and events to the right socket.
pub type ConnId = u64;

/// Generous enough for a full session/inhibitor listing, nowhere near
/// enough for a client to use as a memory-exhaustion vector.
const MAX_MESSAGE_LEN: u32 = 16 * 1024 * 1024;

/// Write one length-prefixed, bincode-encoded message: a 4-byte
/// little-endian length followed by that many payload bytes.
/// Deliberately simple -- no varint, no magic number -- because both
/// ends of this protocol are always mitos-session's own code (the
/// daemon, mitos-gui, and mitos-sessionctl).
pub fn write_message<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> Result<()> {
    let payload = bincode::serialize(msg)?;
    let len = u32::try_from(payload.len())
        .map_err(|_| SessionError::Protocol("message too large to frame".into()))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Read one length-prefixed, bincode-encoded message. Blocks until a
/// full message has arrived or the connection is closed, in which
/// case it returns `SessionError::Disconnected` rather than a generic
/// I/O error -- callers use that to distinguish "peer hung up" from
/// "something is actually wrong."
pub fn read_message<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).map_err(|e| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            SessionError::Disconnected
        } else {
            SessionError::Io(e)
        }
    })?;

    let len = u32::from_le_bytes(len_buf);
    if len > MAX_MESSAGE_LEN {
        return Err(SessionError::Protocol(format!("message of {len} bytes exceeds the {MAX_MESSAGE_LEN} byte limit")));
    }

    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload)?;
    Ok(bincode::deserialize(&payload)?)
}
