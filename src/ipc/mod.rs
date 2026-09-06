//! The wire protocol between mitos-session and its clients
//! (mitos-gui, mitos-sessionctl): message shapes (`messages`), framing
//! (`protocol`), the socket server (`server`), a blocking request/reply
//! client (`client`), and `SO_PEERCRED`-based identity (`permissions`
//! -- the actual allow/deny policy lives in `policy::security_policy`).

mod client;
mod messages;
mod permissions;
mod protocol;
mod server;

pub use client::IpcClient;
pub use messages::{Event, InhibitorInfo, Message, Request, Response, SessionInfo};
pub use permissions::{is_root, peer_credentials, PeerCred};
pub use protocol::{read_message, write_message, ConnId};
pub use server::{spawn, ConnRegistry, ManagerCommand, ManagerMessage};
