#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};

use mitos_session::ipc::protocol::{read_message, write_message};
use mitos_session::session::runtime::ensure_user_runtime_dir_in;
use nix::unistd::{Gid, Uid};

fn running_as_root() -> bool {
    Uid::effective().is_root()
}

// ---------------------------------------------------------------------
// XDG_RUNTIME_DIR tests
// ---------------------------------------------------------------------

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

        let decoded: Vec<u8> = read_message(framed.as_slice()).unwrap();

        assert_eq!(payload, decoded);
    }
}

#[test]
fn ipc_framing_rejects_oversized_length_before_payload_allocation() {
    // Must match MAX_MESSAGE_LEN in src/ipc/protocol.rs.
    const MAX_MESSAGE_LEN: u32 = 16 * 1024 * 1024;

    let mut framed = Vec::new();
    framed.extend_from_slice(&(MAX_MESSAGE_LEN + 1).to_le_bytes());

    let result: mitos_session::errors::Result<Vec<u8>> = read_message(framed.as_slice());

    assert!(result.is_err());
}

#[test]
#[ignore = "requires root"]
fn ipc_framing_stress_roundtrip() {
    if !running_as_root() {
        return;
    }

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

        let decoded: Vec<u8> = read_message(framed.as_slice()).unwrap();

        assert_eq!(payload, decoded);
    }
}

// ---------------------------------------------------------------------
// Unix socket peer credential tests
// ---------------------------------------------------------------------

#[test]
#[ignore = "requires root"]
fn unix_socket_peer_credentials_are_available() {
    if !running_as_root() {
        return;
    }

    use std::os::unix::net::UnixStream;

    let (a, _b) = UnixStream::pair().unwrap();

    let (peer_uid, peer_gid) = nix::sys::socket::getpeereid(&a).unwrap();

    assert_eq!(peer_uid, Uid::effective());
    assert_eq!(peer_gid, Gid::effective());
}

#[test]
#[ignore = "requires root and mitos-pam-test user"]
fn pam_authenticator_validates_real_user() {
    if !running_as_root() { return; }

    let auth = PamAuthenticator::new("mitos-login"); // Must match /etc/pam.d/mitos-login
    let req = AuthRequest {
        session_id: "test-session".into(),
        user_name: "mitos-pam-test".into(),
        password: ZeroizingString::from("correct_password"), // Use actual password
    };

    let outcome = auth.authenticate(&req).unwrap();
    assert!(matches!(outcome, AuthOutcome::Success));

    let bad_req = AuthRequest {
        session_id: "test-session".into(),
        user_name: "mitos-pam-test".into(),
        password: ZeroizingString::from("wrong_password"),
    };

    let outcome = auth.authenticate(&bad_req).unwrap();
    assert!(matches!(outcome, AuthOutcome::Failed));
}

