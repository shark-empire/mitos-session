//! Library crate backing both the `mitos-session` daemon
//! (`src/main.rs`) and the `mitos-sessionctl` CLI (`bin/mitos-sessionctl.rs`),
//! so the two share exactly one copy of the wire protocol (`ipc`) and
//! domain types instead of two independently-maintained definitions
//! that could silently drift apart.

pub mod authentication;
pub mod config;
pub mod errors;
pub mod idle;
pub mod ipc;
pub mod launcher;
pub mod lock;
pub mod logging;
pub mod policy;
pub mod power;
pub mod seat;
pub mod session;
pub mod signals;
pub mod user;
