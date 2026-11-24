//! The sample `config/session.toml` is what ends up in `/etc/mitos/`, so
//! it has to keep parsing -- and keep agreeing with the compiled-in
//! defaults it documents.

use mitos_session::config::{self, Settings};
use std::path::Path;

fn load_shipped() -> Settings {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/session.toml");
    config::load(Some(path.as_path())).unwrap()
}

#[test]
fn shipped_config_matches_the_compiled_in_defaults() {
    let shipped = format!("{:?}", load_shipped());
    let defaults = format!("{:?}", Settings::default());
    assert_eq!(shipped, defaults);
}

#[test]
fn a_missing_config_file_falls_back_to_the_defaults() {
    let missing = Path::new("/nonexistent/mitos/session.toml");
    let loaded = format!("{:?}", config::load(Some(missing)).unwrap());
    let defaults = format!("{:?}", Settings::default());
    assert_eq!(loaded, defaults);
}
