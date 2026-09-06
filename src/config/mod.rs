//! On-disk configuration: schema (`settings`), compiled-in fallbacks
//! (`defaults`), and the logic that finds and parses `session.toml`
//! (`loader`).

mod defaults;
mod loader;
mod settings;

pub use loader::load;
pub use settings::{
    AuthSettings, IdleSettings, IpcSettings, LockSettings, LoggingSettings, PowerSettings,
    SeatSettings, Settings, SessionSettings,
};
