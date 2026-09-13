//! Wire-level tests: framing round trips and `SO_PEERCRED` identity.
//! The socket server/client threading model (`ipc::server`) needs a
//! running calloop loop to exercise meaningfully and is covered by
//! manual/hardware testing per README's roadmap rather than here.

use mitos_session::elevation::{ElevationAction, ElevationResponse, ElevationRisk};
use mitos_session::ipc::{
    peer_credentials, read_message, write_message, Message, Request, Response,
};
use std::os::unix::net::UnixStream;

#[test]
fn request_and_message_frames_round_trip_over_a_socket_pair() {
    let (mut a, mut b) = UnixStream::pair().unwrap();

    write_message(&mut a, &Request::ListSessions).unwrap();
    let received: Request = read_message(&mut b).unwrap();
    assert!(matches!(received, Request::ListSessions));

    write_message(&mut b, &Message::Response(Response::Ok)).unwrap();
    let received: Message = read_message(&mut a).unwrap();
    assert!(matches!(received, Message::Response(Response::Ok)));
}

#[test]
fn elevation_requests_round_trip_including_their_nested_action_payload() {
    let (mut a, mut b) = UnixStream::pair().unwrap();

    let sent = Request::RequestElevation {
        session_id: 7,
        action: ElevationAction {
            requesting_app: "Video Editor".into(),
            description: "Raw Disk Access".into(),
            risk: ElevationRisk::Critical,
            duration_label: "5 minutes".into(),
        },
    };
    write_message(&mut a, &sent).unwrap();
    let received: Request = read_message(&mut b).unwrap();
    match received {
        Request::RequestElevation { session_id, action } => {
            assert_eq!(session_id, 7);
            assert_eq!(action.requesting_app, "Video Editor");
            assert_eq!(action.risk, ElevationRisk::Critical);
        }
        other => panic!("expected RequestElevation, got {other:?}"),
    }
}

#[test]
fn elevation_responses_round_trip_both_the_password_and_cancel_shapes() {
    let (mut a, mut b) = UnixStream::pair().unwrap();

    write_message(
        &mut a,
        &Request::RespondElevation {
            request_id: 1,
            response: ElevationResponse::Password("hunter2".into()),
        },
    )
    .unwrap();
    let received: Request = read_message(&mut b).unwrap();
    assert!(matches!(
        received,
        Request::RespondElevation {
            request_id: 1,
            response: ElevationResponse::Password(p)
        } if p == "hunter2"
    ));

    write_message(
        &mut a,
        &Request::RespondElevation {
            request_id: 2,
            response: ElevationResponse::Cancelled,
        },
    )
    .unwrap();
    let received: Request = read_message(&mut b).unwrap();
    assert!(matches!(
        received,
        Request::RespondElevation {
            request_id: 2,
            response: ElevationResponse::Cancelled
        }
    ));
}

#[test]
fn a_closed_connection_is_reported_as_disconnected_not_a_generic_error() {
    let (a, mut b) = UnixStream::pair().unwrap();
    drop(a);
    let result = read_message::<_, Request>(&mut b);
    assert!(matches!(
        result,
        Err(mitos_session::errors::SessionError::Disconnected)
    ));
}

#[test]
fn peer_credentials_reports_the_current_process() {
    let (a, _b) = UnixStream::pair().unwrap();
    let cred = peer_credentials(&a).unwrap();
    assert_eq!(cred.uid, nix::unistd::Uid::current().as_raw());
}
