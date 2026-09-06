//! The authorization boundary is the one place a bug here is a real
//! vulnerability, so it gets its own dedicated test file rather than
//! being folded into `tests/ipc.rs` or `tests/session.rs`.

use mitos_session::config::SessionSettings;
use mitos_session::ipc::{PeerCred, Request};
use mitos_session::policy::authorize;
use mitos_session::session::{SessionManager, SessionType};
use mitos_session::user::User;
use nix::unistd::{Gid, Uid};
use std::path::{Path, PathBuf};

fn fake_user(uid: u32, home: &Path) -> User {
    User {
        uid: Uid::from_raw(uid),
        gid: Gid::from_raw(uid),
        name: format!("user{uid}"),
        home: home.to_path_buf(),
        shell: PathBuf::from("/bin/sh"),
    }
}

fn peer(uid: u32) -> PeerCred {
    PeerCred {
        uid,
        gid: uid,
        pid: 1,
    }
}

#[test]
fn a_session_owner_may_act_on_it_but_a_stranger_may_not() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = SessionSettings {
        runtime_dir: tmp.path().join("run"),
        ..Default::default()
    };
    let mut mgr = SessionManager::new();

    // Use the test runner's own uid so `ensure_home_ready`'s ownership
    // check succeeds without root.
    let real_uid = Uid::current().as_raw();
    let id = mgr
        .create_session(
            fake_user(real_uid, &tmp.path().join("home")),
            "seat0",
            SessionType::Wayland,
            &settings,
        )
        .unwrap();

    let request = Request::LockSession { session_id: id };
    assert!(authorize(&peer(real_uid), &request, &mgr).is_ok());
    assert!(authorize(&peer(real_uid.wrapping_add(12_345)), &request, &mgr).is_err());
}

#[test]
fn root_bypasses_ownership_checks_entirely() {
    let mgr = SessionManager::new();
    // Session 42 doesn't even exist -- root's request should still be
    // authorized; whether the session exists is a separate concern
    // `SessionManager` reports on its own.
    let request = Request::LockSession { session_id: 42 };
    assert!(authorize(&peer(0), &request, &mgr).is_ok());
}

#[test]
fn system_wide_requests_need_no_session_ownership() {
    let mgr = SessionManager::new();
    assert!(authorize(&peer(1000), &Request::ListSessions, &mgr).is_ok());
    assert!(authorize(&peer(1000), &Request::ListInhibitors, &mgr).is_ok());
    assert!(authorize(&peer(1000), &Request::Suspend, &mgr).is_ok());
}

#[test]
fn only_root_or_the_account_itself_may_start_its_session() {
    let mgr = SessionManager::new();
    // "root" always exists locally, so this doesn't depend on any
    // account being present beyond what every Linux box already has.
    let request = Request::CreateSession {
        user_name: "root".into(),
        seat_id: None,
        session_type: None,
    };

    assert!(authorize(&peer(0), &request, &mgr).is_ok());
    // uid 65534 is the conventional "nobody" account and is never root.
    assert!(authorize(&peer(65_534), &request, &mgr).is_err());
}
