use super::messages::{Event, Message, Request, Response};
use super::permissions::{peer_credentials, PeerCred};
use super::protocol::{read_message, write_message, ConnId};
use crate::errors::{Result, SessionError};
use calloop::channel::Sender as LoopSender;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

/// One parsed request from a connection, on its way to the calloop
/// thread that owns all daemon state.
pub struct ManagerCommand {
    pub conn_id: ConnId,
    pub peer: PeerCred,
    pub request: Request,
}

/// Everything the IPC layer hands to the calloop thread. `Command` is
/// the common case; `Connected`/`Disconnected` exist so the daemon can
/// keep `ConnRegistry` (and "was this the compositor for session N?"
/// bookkeeping) in sync without a separate out-of-band channel.
pub enum ManagerMessage {
    Connected { conn_id: ConnId, peer: PeerCred, outbox: mpsc::Sender<Message> },
    Command(ManagerCommand),
    Disconnected { conn_id: ConnId },
}

/// Maps a live connection to the channel that feeds its writer thread.
/// Owned by the calloop thread; nothing here is shared across threads,
/// which is exactly the point -- see README's "one thread, one brain."
#[derive(Default)]
pub struct ConnRegistry {
    outboxes: HashMap<ConnId, mpsc::Sender<Message>>,
}

impl ConnRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, id: ConnId, outbox: mpsc::Sender<Message>) {
        self.outboxes.insert(id, outbox);
    }

    pub fn unregister(&mut self, id: ConnId) {
        self.outboxes.remove(&id);
    }

    pub fn send_response(&self, id: ConnId, response: Response) {
        if let Some(tx) = self.outboxes.get(&id) {
            let _ = tx.send(Message::Response(response));
        }
    }

    pub fn send_event(&self, id: ConnId, event: Event) {
        if let Some(tx) = self.outboxes.get(&id) {
            let _ = tx.send(Message::Event(event));
        }
    }
}

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

/// Bind the IPC socket and start accepting connections. Each accepted
/// connection gets a reader thread (blocking `read_message` loop,
/// forwarding parsed requests to `cmd_tx`) and a writer thread
/// (drains an per-connection mpsc channel and writes to the socket).
/// Neither thread ever touches daemon state directly -- they only ever
/// move bytes and hand structured messages across a channel.
pub fn spawn(socket_path: &Path, mode: u32, cmd_tx: LoopSender<ManagerMessage>) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(mode))?;
    tracing::info!(path = %socket_path.display(), "IPC socket listening");

    thread::Builder::new()
        .name("mitos-session-ipc-accept".into())
        .spawn(move || accept_loop(listener, cmd_tx))
        .map_err(SessionError::Io)?;
    Ok(())
}

fn accept_loop(listener: UnixListener, cmd_tx: LoopSender<ManagerMessage>) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let cmd_tx = cmd_tx.clone();
                thread::spawn(move || handle_connection(stream, cmd_tx));
            }
            Err(e) => tracing::warn!(error = %e, "failed to accept IPC connection"),
        }
    }
}

fn handle_connection(stream: UnixStream, cmd_tx: LoopSender<ManagerMessage>) {
    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let peer = match peer_credentials(&stream) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "rejecting connection: could not read peer credentials");
            return;
        }
    };

    let mut reader = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to clone IPC connection for reading");
            return;
        }
    };
    let mut writer = stream;

    let (out_tx, out_rx) = mpsc::channel::<Message>();

    // Register before reading anything, so a request that produces an
    // immediate reply can never race ahead of this connection existing
    // in `ConnRegistry`.
    if cmd_tx
        .send(ManagerMessage::Connected { conn_id, peer, outbox: out_tx.clone() })
        .is_err()
    {
        return; // daemon is shutting down
    }

    let writer_handle = thread::spawn(move || {
        for msg in out_rx {
            if write_message(&mut writer, &msg).is_err() {
                break;
            }
        }
    });

    loop {
        match read_message::<_, Request>(&mut reader) {
            Ok(request) => {
                let cmd = ManagerCommand { conn_id, peer, request };
                if cmd_tx.send(ManagerMessage::Command(cmd)).is_err() {
                    break;
                }
            }
            Err(SessionError::Disconnected) => break,
            Err(e) => {
                tracing::debug!(error = %e, conn_id, "malformed IPC message, closing connection");
                break;
            }
        }
    }

    drop(out_tx); // lets the writer thread's `for msg in out_rx` end
    let _ = writer_handle.join();
    let _ = cmd_tx.send(ManagerMessage::Disconnected { conn_id });
}
