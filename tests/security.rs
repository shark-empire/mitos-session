//! The authorization boundary is the one place a bug here is a real
//! vulnerability, so it gets its own dedicated test file rather than
//! being folded into `tests/ipc.rs` or `tests/session.rs`.

use mitos_session::config::SessionSettings;
use mitos_session::elevation::{
    ElevationAction, ElevationManager, ElevationResponse, ElevationRisk,
};
use mitos_session::ipc::{PeerCred, Request};
use mitos_session::policy::authorize;
use mitos_session::session::{SessionManager, SessionType};
use mitos_session::user::User;
use nix::unistd::{Gid, Uid};
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// No test in this file (other than the elevation-specific ones)
/// exercises elevation at all -- an empty manager, with no configured
/// requester, is a fine stand-in wherever `authorize` just needs
/// *some* `&ElevationManager` to compile against.
fn no_elevation() -> ElevationManager {
    ElevationManager::new()
}

fn elevation_action() -> ElevationAction {
    ElevationAction {
        requesting_app: "Video Editor".into(),
        description: "Raw Disk Access".into(),
        risk: ElevationRisk::Critical,
        duration_label: "5 minutes".into(),
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
    let elevation = no_elevation();
    assert!(authorize(&peer(real_uid), 1, &request, &mgr, &elevation).is_ok());
    assert!(authorize(
        &peer(real_uid.wrapping_add(12_345)),
        1,
        &request,
        &mgr,
        &elevation
    )
    .is_err());
}

#[test]
fn root_bypasses_ownership_checks_entirely() {
    let mgr = SessionManager::new();
    // Session 42 doesn't even exist -- root's request should still be
    // authorized; whether the session exists is a separate concern
    // `SessionManager` reports on its own.
    let request = Request::LockSession { session_id: 42 };
    assert!(authorize(&peer(0), 1, &request, &mgr, &no_elevation()).is_ok());
}

#[test]
fn system_wide_requests_need_no_session_ownership() {
    let mgr = SessionManager::new();
    let elevation = no_elevation();
    assert!(authorize(&peer(1000), 1, &Request::ListSessions, &mgr, &elevation).is_ok());
    assert!(authorize(&peer(1000), 1, &Request::ListInhibitors, &mgr, &elevation).is_ok());
    assert!(authorize(&peer(1000), 1, &Request::Suspend, &mgr, &elevation).is_ok());
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

    let elevation = no_elevation();
    assert!(authorize(&peer(0), 1, &request, &mgr, &elevation).is_ok());
    // uid 65534 is the conventional "nobody" account and is never root.
    assert!(authorize(&peer(65_534), 1, &request, &mgr, &elevation).is_err());
}

#[test]
fn only_the_configured_elevation_service_or_root_may_request_elevation() {
    let mgr = SessionManager::new();
    let mut elevation = no_elevation();
    elevation.set_requester_uid(Some(9000));
    let request = Request::RequestElevation {
        session_id: 1,
        action: elevation_action(),
    };

    assert!(authorize(&peer(9000), 1, &request, &mgr, &elevation).is_ok());
    assert!(authorize(&peer(0), 1, &request, &mgr, &elevation).is_ok());
    // The session's own owner is *not* automatically allowed to open
    // an elevation prompt on itself -- that would let any app trigger
    // a system password prompt with text of its own choosing.
    assert!(authorize(&peer(1234), 1, &request, &mgr, &elevation).is_err());
}

#[test]
fn elevation_requests_are_refused_when_no_service_account_is_configured() {
    let mgr = SessionManager::new();
    let elevation = no_elevation(); // requester_uid: None
    let request = Request::RequestElevation {
        session_id: 1,
        action: elevation_action(),
    };
    // Fails closed: with nothing configured, nobody but root can open
    // a prompt, rather than defaulting open to "any uid".
    assert!(authorize(&peer(9000), 1, &request, &mgr, &elevation).is_err());
    assert!(authorize(&peer(0), 1, &request, &mgr, &elevation).is_ok());
}

#[test]
fn only_the_expected_compositor_connection_or_root_may_answer_a_prompt() {
    let mgr = SessionManager::new();
    let mut elevation = no_elevation();
    elevation.set_requester_uid(Some(9000));
    let policy = mitos_session::elevation::ElevationPolicy {
        enabled: true,
        prompt_timeout: std::time::Duration::from_secs(120),
        max_pending_per_session: 3,
    };
    let auth_policy = mitos_session::authentication::AuthPolicy {
        pam_service: "test".into(),
        allow_empty_password: false,
        max_attempts: 3,
        lockout: std::time::Duration::from_secs(30),
    };
    // conn 42 is the compositor this prompt was actually shown to;
    // conn 99 is some other, unrelated connection.
    let request_id = elevation
        .begin(1, 42, 100, &policy, &auth_policy, Instant::now())
        .unwrap();
    let request = Request::RespondElevation {
        request_id,
        response: ElevationResponse::Cancelled,
    };

    assert!(authorize(&peer(1234), 42, &request, &mgr, &elevation).is_ok());
    assert!(authorize(&peer(1234), 99, &request, &mgr, &elevation).is_err());
    // Root may answer on any connection, mirroring root's universal
    // bypass everywhere else in this function.
    assert!(authorize(&peer(0), 99, &request, &mgr, &elevation).is_ok());
}

#[test]
fn responding_to_an_unknown_request_id_is_denied_like_a_wrong_connection() {
    let mgr = SessionManager::new();
    let elevation = no_elevation();
    let request = Request::RespondElevation {
        request_id: 999,
        response: ElevationResponse::Cancelled,
    };
    // No pending request 999 exists at all -- same error shape as
    // "wrong connection", on purpose (see `authorize`'s doc comment).
    assert!(authorize(&peer(1234), 42, &request, &mgr, &elevation).is_err());
}
