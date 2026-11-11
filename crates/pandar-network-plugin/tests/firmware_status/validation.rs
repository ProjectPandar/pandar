use super::support::*;

pub(crate) fn never_populated_marker_does_not_fabricate_reset() {
    let start = Instant::now();
    let mut cache = FirmwareStatusCache::new(16);
    cache
        .observe_printers_at(
            &batch_projection(Some(marker_firmware("session-1", 5))),
            16,
            1,
            start,
        )
        .unwrap();

    assert!(cache.next_status_override_at("SERIAL", start).is_none());
}
