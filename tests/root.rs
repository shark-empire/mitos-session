#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::time::Duration;

use mitos_session::authentication::{check, AuthOutcome, AuthPolicy, AuthRequest, PamAuthenticator};
use mitos_session::errors::SessionError;
use mitos_session::ipc::{peer_credentials, read_message, write_message};
use mitos_session::session::runtime::ensure_user_runtime_dir_in;
use nix::unistd::{Gid, Uid};

fn running_as_root() -> bool {
    Uid::effective().is_root()
}

// ---------------------------------------------------------------------
// XDG_RUNTIME_DIR tests
// ---------------------------------------------------------------------

#[test]
fn runtime_dir_is_created_for_the_current_user_without_root() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("user");

    let uid = Uid::current();
    let gid = Gid::current();

    let dir = ensure_user_runtime_dir_in(&base, uid, gid).unwrap();

    let meta = fs::metadata(&dir).unwrap();

    assert!(meta.is_dir());
    assert_eq!(meta.uid(), uid.as_raw());
    assert_eq!(meta.gid(), gid.as_raw());
    assert_eq!(meta.mode() & 0o777, 0o700);
}

#[test]
fn runtime_dir_refuses_a_symlink_without_root() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("user");
    fs::create_dir_all(&base).unwrap();

    let uid = Uid::current();
    let gid = Gid::current();

    let target = tmp.path().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    symlink(&target, base.join(uid.as_raw().to_string())).unwrap();

    assert!(ensure_user_runtime_dir_in(&base, uid, gid).is_err());
}

#[test]
#[ignore = "requires root"]
fn runtime_dir_is_created_with_secure_ownership_and_permissions() {
    if !running_as_root() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("user");

    let uid = Uid::from_raw(65432);
    let gid = Gid::from_raw(65432);

    let dir = ensure_user_runtime_dir_in(&base, uid, gid).unwrap();

    let meta = fs::metadata(&dir).unwrap();

    assert!(meta.is_dir());
    assert_eq!(meta.uid(), 65432);
    assert_eq!(meta.gid(), 65432);
    assert_eq!(meta.mode() & 0o777, 0o700);
}

#[test]
#[ignore = "requires root"]
fn runtime_dir_refuses_symlink_attack() {
    if !running_as_root() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("user");

    fs::create_dir_all(&base).unwrap();

    let uid = Uid::from_raw(65432);
    let gid = Gid::from_raw(65432);

    let evil_target = tmp.path().join("evil-target");
    fs::create_dir_all(&evil_target).unwrap();

    let link = base.join("65432");
    symlink(&evil_target, &link).unwrap();

    let result = ensure_user_runtime_dir_in(&base, uid, gid);

    assert!(
        result.is_err(),
        "runtime directory setup must refuse symlinks"
    );
}

// ---------------------------------------------------------------------
// IPC framing tests
// ---------------------------------------------------------------------

#[test]
fn ipc_framing_roundtrip_small_messages() {
    for len in [0usize, 1, 2, 3, 4, 5, 16, 64, 255, 256, 1024] {
        let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

        let mut framed = Vec::new();
        write_message(&mut framed, &payload).unwrap();

        let mut reader = framed.as_slice();
        let decoded: Vec<u8> = read_message(&mut reader).unwrap();

        assert_eq!(payload, decoded);
    }
}

#[test]
fn ipc_framing_rejects_oversized_length_before_payload_allocation() {
    // A length prefix far beyond any sane limit and no payload behind
    // it at all: the reader must refuse it up front instead of trying
    // to allocate (or wait for) that many bytes.
    let framed = u32::MAX.to_le_bytes();
    let mut reader = framed.as_slice();

    let result: mitos_session::errors::Result<Vec<u8>> = read_message(&mut reader);

    assert!(matches!(result, Err(SessionError::Protocol(_))));
}

#[test]
#[ignore = "slow stress test; run with --ignored"]
fn ipc_framing_stress_roundtrip() {
    // Deterministic xorshift-style PRNG so the test is reproducible.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 7;
            x ^= x >> 9;
            self.0 = x;
            x
        }
    }

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    for _ in 0..10_000 {
        let len = (rng.next() % 8192) as usize;

        let payload: Vec<u8> = (0..len).map(|_| (rng.next() % 256) as u8).collect();

        let mut framed = Vec::new();
        write_message(&mut framed, &payload).unwrap();

        let mut reader = framed.as_slice();
        let decoded: Vec<u8> = read_message(&mut reader).unwrap();

        assert_eq!(payload, decoded);
    }
}

// ---------------------------------------------------------------------
// Unix socket peer credential tests
// ---------------------------------------------------------------------

#[test]
fn unix_socket_peer_credentials_are_available() {
    use std::os::unix::net::UnixStream;

    let (a, _b) = UnixStream::pair().unwrap();

    let cred = peer_credentials(&a).unwrap();

    assert_eq!(cred.uid, Uid::effective().as_raw());
    assert_eq!(cred.gid, Gid::effective().as_raw());
}

#[test]
#[ignore = "requires root and mitos-pam-test user"]
fn pam_authenticator_validates_real_user() {
    if !running_as_root() {
        return;
    }

    let policy = AuthPolicy {
        pam_service: "mitos-login".into(), // Must match /etc/pam.d/mitos-login
        allow_empty_password: false,
        max_attempts: 3,
        lockout: Duration::from_secs(30),
    };
    let auth = PamAuthenticator::new(&policy);
    let mut attempts = 0;

    let good_request = AuthRequest {
        session_id: 1,
        user_name: "mitos-pam-test".into(),
        password: "correct_password".into(), // Use actual password
    };
    let outcome = check(&auth, &good_request, &policy, &mut attempts);
    assert_eq!(outcome, AuthOutcome::Success);

    let bad_request = AuthRequest {
        session_id: 1,
        user_name: "mitos-pam-test".into(),
        password: "wrong_password".into(),
    };
    let outcome = check(&auth, &bad_request, &policy, &mut attempts);
    assert!(matches!(outcome, AuthOutcome::Failure { .. }));
}
