use std::time::Duration;

use pandar_network_plugin::firmware::{FirmwareCallback, FirmwareCallbackQueue, FirmwareTunnel};

#[test]
fn firmware_callback_windows_are_anchored_to_each_originating_handoff() {
    let queue = FirmwareCallbackQueue::new();
    let start = std::time::Instant::now();
    let delayed = queue
        .push(
            4,
            FirmwareCallback {
                dev_id: "DELAYED".into(),
                tunnel: FirmwareTunnel::Cloud,
                message: r#"{"upgrade":{"sequence_id":"one"}}"#.into(),
            },
        )
        .unwrap();
    let overlap = queue
        .push(
            4,
            FirmwareCallback {
                dev_id: "OVERLAP".into(),
                tunnel: FirmwareTunnel::Local,
                message: r#"{"upgrade":{"sequence_id":"two"}}"#.into(),
            },
        )
        .unwrap();

    assert!(queue.return_handoff_at(overlap, 200, 41, 73, start));
    assert!(
        queue
            .take_ready_at(start + Duration::from_millis(1_099))
            .is_none()
    );
    assert!(queue.return_handoff_at(delayed, 100, 0, 0, start + Duration::from_millis(500)));

    let ready = queue
        .take_ready_at(start + Duration::from_millis(1_100))
        .expect("overlapping callback becomes ready at its own handoff +1.1s");
    assert_eq!(ready.token, overlap);
    assert_eq!(ready.origin_tick, 200);
    assert_eq!(ready.local_generation, 41);
    assert_eq!(ready.cache_generation, 73);
    assert_eq!(ready.dev_id, "OVERLAP");
    assert_eq!(ready.tunnel, FirmwareTunnel::Local);
    assert_eq!(ready.message, r#"{"upgrade":{"sequence_id":"two"}}"#);
    assert!(
        queue
            .take_ready_at(start + Duration::from_millis(1_599))
            .is_none()
    );

    let ready = queue
        .take_ready_at(start + Duration::from_millis(1_600))
        .expect("delayed callback becomes ready at its own handoff +1.1s");
    assert_eq!(ready.token, delayed);
    assert_eq!(ready.origin_tick, 100);
    assert_eq!(ready.local_generation, 0);
    assert_eq!(ready.cache_generation, 0);
    assert_eq!(ready.dev_id, "DELAYED");
    assert_eq!(ready.tunnel, FirmwareTunnel::Cloud);
    assert_eq!(ready.message, r#"{"upgrade":{"sequence_id":"one"}}"#);
}

#[test]
fn firmware_callback_is_ineligible_before_handoff_and_expires_at_two_seconds() {
    let queue = FirmwareCallbackQueue::new();
    let start = std::time::Instant::now();
    let never_handed_off = queue
        .push(
            1,
            FirmwareCallback {
                dev_id: "A".into(),
                tunnel: FirmwareTunnel::Cloud,
                message: "never".into(),
            },
        )
        .unwrap();
    assert!(
        queue
            .take_ready_at(start + Duration::from_secs(60))
            .is_none()
    );
    assert!(queue.return_handoff_at(never_handed_off, 1, 0, 0, start));
    assert!(
        queue
            .take_ready_at(start + Duration::from_millis(2_000))
            .is_none()
    );
    assert!(!queue.return_handoff_at(never_handed_off, 2, 0, 0, start));
}
