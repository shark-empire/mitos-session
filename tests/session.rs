//! Exercises `SessionManager` end to end: creating a session actually
//! touches the filesystem (home directory checks, XDG_RUNTIME_DIR
//! creation), so these run as integration tests against a real
//! tempdir rather than as unit tests inside the crate.

use mitos_session::config::SessionSettings;
use mitos_session::session::{SessionManager, SessionType};
use mitos_session::user::User;
use nix::unistd::{Gid, Uid};
use std::path::{Path, PathBuf};

fn fake_user(home: &Path) -> User {
    User {
        // Use the test runner's own uid/gid so the ownership check and
        // chown in `ensure_home_ready` succeed without root.
        uid: Uid::current(),
        gid: Gid::current(),
        name: "testuser".to_string(),
        home: home.to_path_buf(),
        shell: PathBuf::from("/bin/sh"),
    }
}

fn settings_in(tmp: &Path) -> SessionSettings {
    SessionSettings {
        runtime_dir: tmp.join("run"),
        ..Default::default()
    }
}

#[test]
fn create_and_terminate_session() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = settings_in(tmp.path());
    let mut mgr = SessionManager::new();

    let id = mgr
        .create_session(fake_user(&tmp.path().join("home")), "seat0", SessionType::Wayland, &settings)
        .expect("session should be created");

    let ctx = mgr.get(id).unwrap();
    assert_eq!(ctx.session.seat_id, "seat0");
    assert_eq!(ctx.session.session_type, SessionType::Wayland);

    mgr.terminate_session(id).unwrap();
    assert!(mgr.get(id).is_err());
}

#[test]
fn respects_max_sessions_per_user() {
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = settings_in(tmp.path());
    settings.max_sessions_per_user = 1;
    let mut mgr = SessionManager::new();

    mgr.create_session(fake_user(&tmp.path().join("home")), "seat0", SessionType::Wayland, &settings)
        .unwrap();
    let second = mgr.create_session(fake_user(&tmp.path().join("home")), "seat0", SessionType::Wayland, &settings);
    assert!(second.is_err());
}

#[test]
fn unknown_session_id_is_reported_distinctly() {
    let mgr = SessionManager::new();
    assert!(mgr.get(999).is_err());
}
