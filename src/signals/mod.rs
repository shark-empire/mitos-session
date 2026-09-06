//! SIGTERM/SIGINT/SIGHUP/SIGCHLD handling. `main` registers
//! `handlers::WATCHED` with calloop directly and calls
//! `handlers::classify` inside its own callback -- kept this thin
//! (rather than a generic `register()` wrapper) since calloop's signal
//! source is exactly the kind of API surface most likely to have
//! shifted between minor versions and worth keeping easy to eyeball
//! against whatever version actually gets pulled in.

mod handlers;
mod shutdown;

pub use handlers::{classify, SignalEvent, WATCHED};
pub use shutdown::graceful_shutdown;
