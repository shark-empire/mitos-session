use crate::errors::{Result, SessionError};
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use std::os::unix::net::UnixStream;

/// Credentials the kernel itself vouches for (`SO_PEERCRED`), not
/// anything the peer claims about itself. This is the only identity
/// mitos-session trusts an IPC client with -- there is no separate
/// login/auth handshake on the socket itself.
#[derive(Debug, Clone, Copy)]
pub struct PeerCred {
    pub uid: u32,
    pub gid: u32,
    pub pid: i32,
}

pub fn peer_credentials(stream: &UnixStream) -> Result<PeerCred> {
    let cred = getsockopt(stream, PeerCredentials)
        .map_err(|e| SessionError::Protocol(format!("SO_PEERCRED failed: {e}")))?;
    Ok(PeerCred {
        uid: cred.uid(),
        gid: cred.gid(),
        pid: cred.pid(),
    })
}

pub fn is_root(cred: &PeerCred) -> bool {
    cred.uid == 0
}
