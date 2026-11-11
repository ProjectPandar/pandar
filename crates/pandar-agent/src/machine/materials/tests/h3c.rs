use super::*;

// Shape of a live H3C (N10) push, firmware 01.01.03.00 (bambuddy
// test_h3c_ams3h.py): one AMS 3H (unit 32, Studio type 6) whose slot k feeds
// hotend k, slot 7 empty, `tray_now` as the raw slot, and one external input
// per hotend (vir_slot 247-255).
fn h3c_report() -> Value {
    let mut trays = (0..9)
        .map(|slot| {
            serde_json::json!({
                "id": slot.to_string(),
                "state": 27,
                "tray_type": "PLA",
                "tray_color": "DE4343FF",
                "tray_info_idx": "GFL99"
            })
        })
        .collect::<Vec<_>>();
    trays[7] = serde_json::json!({"id": "7", "state": 160});
    let vir_slot = (247..=255)
        .map(|id| {
            serde_json::json!({
                "id": id.to_string(),
                "state": 1,
                "tray_type": if id == 250 { "PETG" } else { "" },
                "tray_color": "00000000"
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({"print": {
        "ams": {
            "tray_now": "6",
            "tray_exist_bits": "17f0000000",
            "ams_exist_bits": "2000",
            "ams": [{"id": "32", "info": "240842026", "tray": trays}]
        },
        "vir_slot": vir_slot
    }})
}

#[test]
fn h3c_ams_3h_uses_studio_rack_tray_ids_exist_bits_and_raw_active_slot() {
    let patch = normalize(h3c_report()).unwrap();
    let unit = &patch.ams_units[0];

    assert_eq!(unit.unit_kind.as_deref(), Some("ams_3h"));
    assert_eq!(unit.replace_trays, Some(true));
    assert_eq!(
        unit.trays
            .iter()
            .map(|tray| (tray.tray_id.clone(), tray.global_tray_id, tray.exists))
            .collect::<Vec<_>>(),
        (0..9_u64)
            .map(|slot| (slot.to_string(), Some(28 + slot), Some(slot != 7)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        patch.active_tray,
        Some(TestActiveTray::Ams {
            global_tray_id: 34,
            ams_id: Some("32".to_owned()),
            tray_id: Some("6".to_owned()),
        })
    );
}

#[test]
fn h3c_keeps_one_external_input_per_hotend() {
    let patch = normalize(h3c_report()).unwrap();
    let externals = patch.external_spools.unwrap();

    assert_eq!(patch.replace_external_spools, Some(true));
    assert_eq!(
        externals
            .iter()
            .map(|spool| spool.external_id.as_str())
            .collect::<Vec<_>>(),
        [
            "247", "248", "249", "250", "251", "252", "253", "254", "255"
        ]
    );
    assert_eq!(externals[3].material_type.as_deref(), Some("PETG"));
}
