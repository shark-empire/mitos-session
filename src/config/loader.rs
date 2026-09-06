use super::settings::Settings;
use crate::errors::Result;
use std::env;
use std::path::{Path, PathBuf};

const SYSTEM_CONFIG_PATH: &str = "/etc/mitos/session.toml";
const CONFIG_ENV_VAR: &str = "MITOS_SESSION_CONFIG";

/// Resolve which config file to read, in priority order:
/// 1. an explicit path passed on the command line
/// 2. `$MITOS_SESSION_CONFIG`
/// 3. `/etc/mitos/session.toml`
///
/// If none of those exist, `Settings::default()` is used -- a missing
/// config file is not an error, it just means "use the defaults."
pub fn load(explicit_path: Option<&Path>) -> Result<Settings> {
    let path = explicit_path
        .map(PathBuf::from)
        .or_else(|| env::var_os(CONFIG_ENV_VAR).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(SYSTEM_CONFIG_PATH));

    if !path.exists() {
        tracing::debug!(?path, "no config file found, using compiled-in defaults");
        return Ok(Settings::default());
    }

    let raw = std::fs::read_to_string(&path)?;
    let settings: Settings = toml::from_str(&raw)?;
    tracing::info!(?path, "loaded configuration");
    Ok(settings)
}
