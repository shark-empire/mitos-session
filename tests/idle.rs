use mitos_session::config::IdleSettings;
use mitos_session::idle::{IdleDetector, IdlePolicy, IdleStage};
use std::time::{Duration, Instant};

#[test]
fn policy_converts_seconds_to_durations() {
    let settings = IdleSettings { dim_after_secs: 30, lock_after_secs: 90, suspend_after_secs: 0 };
    let policy = IdlePolicy::from(&settings);

    assert_eq!(policy.dim_after, Duration::from_secs(30));
    assert_eq!(policy.lock_after, Duration::from_secs(90));
    // A zero threshold means "never" -- SuspendRequested should be unreachable.
    assert_eq!(policy.stage_for(Duration::from_secs(10_000)), IdleStage::LockRequested);
}

#[test]
fn independent_seats_track_independently() {
    let policy = IdlePolicy {
        dim_after: Duration::from_secs(5),
        lock_after: Duration::from_secs(10),
        suspend_after: Duration::from_secs(0),
    };
    let mut detector = IdleDetector::new();
    let t0 = Instant::now();
    detector.record_activity("seat0", t0);
    detector.record_activity("seat1", t0);

    // seat1 stays active via a fresh activity report; only seat0 idles out.
    detector.record_activity("seat1", t0 + Duration::from_secs(4));
    let changes = detector.tick(t0 + Duration::from_secs(6), &policy);

    assert_eq!(changes, vec![("seat0".to_string(), IdleStage::Dimmed)]);
    assert_eq!(detector.stage("seat1"), IdleStage::Active);
}
