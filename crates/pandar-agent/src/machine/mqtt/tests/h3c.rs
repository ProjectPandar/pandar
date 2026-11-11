use super::*;

// Nozzle entries of a live H3C (N10) push, firmware 01.01.03.00 (bambuddy
// test_h3c_ams3h.py): rack hotends report 16 + their rack position, the one on
// the toolhead reports id 0 with `addr_id` = the rack position it came from.
#[test]
fn h3c_report_keeps_nine_position_rack_and_mounted_hotend_address() {
    let mut endpoint = endpoint();
    endpoint.model = Some("N10".to_owned());
    let nozzle = |id: u32, addr_id: u32, kind: &str| {
        serde_json::json!({
            "id": id,
            "addr_id": addr_id,
            "stat": 160,
            "type": kind,
            "diameter": 0.4,
            "color_m": "61C680FF",
            "fila_id": "GFL99",
            "wear": 0.0
        })
    };
    let report = serde_json::json!({"print": {"device": {"nozzle": {
        "info": [
            nozzle(16, 0, "HS01"),
            nozzle(0, 6, "HS01"),
            nozzle(23, 7, "HT01"),
            nozzle(24, 8, "HS01"),
        ]
    }}}});

    let system = snapshot_from_json_for(&endpoint, report)
        .nozzle_system
        .unwrap();

    assert_eq!(
        system
            .nozzle
            .info
            .iter()
            .map(|nozzle| (nozzle.id, nozzle.addr_id))
            .collect::<Vec<_>>(),
        [(0, Some(6)), (16, Some(0)), (23, Some(7)), (24, Some(8))]
    );
}
