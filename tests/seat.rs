use mitos_session::seat::SeatManager;

#[test]
fn first_session_on_a_seat_becomes_active() {
    let mut mgr = SeatManager::new();
    mgr.attach_session("seat0", 10);
    assert_eq!(mgr.active_session("seat0").unwrap(), Some(10));
}

#[test]
fn queued_session_is_promoted_after_active_one_logs_out() {
    let mut mgr = SeatManager::new();
    mgr.attach_session("seat0", 10);
    mgr.attach_session("seat0", 20);
    assert_eq!(mgr.active_session("seat0").unwrap(), Some(10));

    mgr.detach_session(10);
    assert_eq!(mgr.active_session("seat0").unwrap(), Some(20));
}

#[test]
fn switch_active_promotes_the_requested_session() {
    let mut mgr = SeatManager::new();
    mgr.attach_session("seat0", 1);
    mgr.attach_session("seat0", 2);

    let previous = mgr.switch_active("seat0", 2).unwrap();
    assert_eq!(previous, Some(1));
    assert_eq!(mgr.active_session("seat0").unwrap(), Some(2));
}

#[test]
fn switching_to_a_session_not_on_that_seat_is_an_error() {
    let mut mgr = SeatManager::new();
    mgr.attach_session("seat0", 1);
    assert!(mgr.switch_active("seat0", 999).is_err());
}

#[test]
fn unknown_seat_is_reported_rather_than_silently_created() {
    let mgr = SeatManager::new();
    assert!(mgr.seat("seat9").is_err());
}
