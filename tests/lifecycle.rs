//! `session::lifecycle` itself is a private implementation detail of
//! `SessionManager` (see src/session/mod.rs) -- these tests exercise
//! its observable effects (environment setup, runtime dir creation,
//! state transitions) through the public API instead.

use mitos_session::config::SessionSettings;
use mitos_session::session::{SessionManager, SessionState, SessionType};
use mitos_session::user::User;
use nix::unistd::{Gid, Uid};
use std::path::{Path, PathBuf};

fn fake_user(home: &Path) -> User {
    User {
        uid: Uid::current(),
        gid: Gid::current(),
        name: "lifecycle-test".to_string(),
        home: home.to_path_buf(),
        shell: PathBuf::from("/bin/sh"),
    }
}

#[test]
fn begin_creates_runtime_dir_in_starting_state() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = SessionSettings {
        runtime_dir: tmp.path().join("run"),
        ..Default::default()
    };
    let mut mgr = SessionManager::new();

    let id = mgr
        .create_session(
            fake_user(&tmp.path().join("home")),
            "seat0",
            SessionType::Wayland,
            &settings,
        )
        .unwrap();
    let ctx = mgr.get(id).unwrap();

    assert_eq!(ctx.state, SessionState::Starting);
    let runtime_dir = ctx
        .environment
        .get("XDG_RUNTIME_DIR")
        .expect("XDG_RUNTIME_DIR should be set");
    assert!(
        Path::new(runtime_dir).is_dir(),
        "runtime dir should have been created on disk"
    );
}

#[test]
fn state_machine_enforces_the_documented_transitions() {
    assert!(SessionState::Starting
        .transition(SessionState::Active)
        .is_ok());
    assert!(SessionState::Active
        .transition(SessionState::Locked)
        .is_ok());
    assert!(SessionState::Locked
        .transition(SessionState::Active)
        .is_ok());
    assert!(SessionState::Active
        .transition(SessionState::Closing)
        .is_ok());
    assert!(SessionState::Closing
        .transition(SessionState::Closed)
        .is_ok());

    // Can't skip straight from Active to Closed, and Closed is terminal.
    assert!(SessionState::Active
        .transition(SessionState::Closed)
        .is_err());
    assert!(SessionState::Closed
        .transition(SessionState::Active)
        .is_err());
}

#[test]
fn terminate_removes_the_session_from_the_registry() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = SessionSettings {
        runtime_dir: tmp.path().join("run"),
        ..Default::default()
    };
    let mut mgr = SessionManager::new();

    let id = mgr
        .create_session(
            fake_user(&tmp.path().join("home")),
            "seat0",
            SessionType::Wayland,
            &settings,
        )
        .unwrap();
    mgr.terminate_session(id).unwrap();
    assert!(mgr.get(id).is_err());
    // Terminating twice is an error, not a panic.
    assert!(mgr.terminate_session(id).is_err());
}
